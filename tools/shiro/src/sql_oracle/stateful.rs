// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Maintenance versus full recomputation on an unindexed reference table.
use super::*;
use hawdb::executor::Row;
use hawdb::QueryOutput;

mod generator;

pub(crate) const PROTOCOL: &str = "shiro-sql-maintenance-v1";

#[derive(Clone, Debug, PartialEq, Eq)]
struct Step {
    name: &'static str,
    mutations: Vec<SqlMutation>,
    rollback: bool,
    expected_error: Option<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Relation {
    name: &'static str,
    actual: SqlQueryInvocation,
    reference: SqlQueryInvocation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Case {
    seed: u64,
    setup: Vec<SqlMutation>,
    steps: Vec<Step>,
    relations: Vec<Relation>,
}

impl Case {
    fn replay(&self) -> JsonValue {
        json!({
            "protocol": PROTOCOL, "seed": self.seed,
            "setup": self.setup.iter().map(SqlMutation::json).collect::<Vec<_>>(),
            "steps": self.steps.iter().map(|step| json!({
                "expected_error": step.expected_error,
                "name": step.name, "transaction": if step.rollback || step.expected_error.is_some() { "rollback" } else { "commit" },
                "mutations": step.mutations.iter().map(SqlMutation::json).collect::<Vec<_>>()
            })).collect::<Vec<_>>(),
            "relations": self.relations.iter().map(|relation| json!({
                "name": relation.name, "actual": relation.actual.json(),
                "reference": relation.reference.json()
            })).collect::<Vec<_>>()
        })
    }
}

#[derive(Debug)]
struct Failure {
    signature: String,
    evidence: JsonValue,
}

fn error(phase: &str, error: HawDBError) -> Failure {
    Failure {
        signature: format!("{phase}_{}", error_class(&error)),
        evidence: json!({"error": error.to_string()}),
    }
}

fn compare(
    phase: &str,
    relation: &Relation,
    actual: &[Row],
    expected: &[Row],
) -> Result<(), Failure> {
    compare_rows(actual, expected, relation.actual.result_semantics).map_err(|reason| Failure {
        signature: format!("{phase}_{}_rows", relation.name),
        evidence: json!({
            "reason": reason, "actual_query": relation.actual.json(),
            "reference_query": relation.reference.json(),
            "actual": actual.iter().map(row_json).collect::<Vec<_>>(),
            "expected": expected.iter().map(row_json).collect::<Vec<_>>()
        }),
    })
}

fn observe(
    relations: &[Relation],
    phase: &str,
    mut query: impl FnMut(&str, &[Value]) -> hawdb::Result<QueryOutput>,
) -> Result<Vec<Vec<Row>>, Failure> {
    relations
        .iter()
        .map(|relation| {
            let actual = query(&relation.actual.sql, &relation.actual.parameters)
                .map_err(|err| error(&format!("{phase}_{}_actual", relation.name), err))?;
            let reference = query(&relation.reference.sql, &relation.reference.parameters)
                .map_err(|err| error(&format!("{phase}_{}_reference", relation.name), err))?;
            let actual = actual.rows.into_rows();
            compare(phase, relation, &actual, &reference.rows.into_rows()).map_err(
                |mut failure| {
                    for (name, invocation) in [
                        ("actual_plan", &relation.actual),
                        ("reference_plan", &relation.reference),
                    ] {
                        failure.evidence[name] = match query(
                            &format!("EXPLAIN {}", invocation.sql),
                            &invocation.parameters,
                        ) {
                            Ok(plan) => json!(plan
                                .rows
                                .into_rows()
                                .iter()
                                .map(row_json)
                                .collect::<Vec<_>>()),
                            Err(err) => json!({"error": err.to_string()}),
                        };
                    }
                    failure
                },
            )?;
            Ok(actual)
        })
        .collect()
}

fn unchanged(
    relations: &[Relation],
    phase: &str,
    before: &[Vec<Row>],
    after: &[Vec<Row>],
) -> Result<(), Failure> {
    for ((relation, before), after) in relations.iter().zip(before).zip(after) {
        compare(phase, relation, after, before)?;
    }
    Ok(())
}

#[derive(Default)]
struct Progress {
    completed_steps: usize,
    operations: BTreeMap<&'static str, usize>,
    index_range_plans: usize,
    fallback_plans: usize,
}

fn run(case: &Case, progress: &mut Progress) -> Result<(), Failure> {
    let mut db = Database::new();
    for mutation in &case.setup {
        db.query_sql_with_params(&mutation.sql, &mutation.parameters)
            .map_err(|err| error("setup", err))?;
    }
    observe(&case.relations, "initial", |sql, params| {
        db.query_sql_with_params(sql, params)
    })?;
    // Plan selection is telemetry, not evidence that two equal executions used
    // different access paths. Small/empty relations may legitimately scan.
    for relation in case
        .relations
        .iter()
        .filter(|relation| relation.name.starts_with("range_"))
    {
        let plan = db
            .query_sql_with_params(
                &format!("EXPLAIN {}", relation.actual.sql),
                &relation.actual.parameters,
            )
            .map_err(|err| error("explain", err))?;
        if plan.rows.iter().any(|row| {
            matches!(row.get("id"), Some(Value::String(id)) if id.contains("IndexRangeScanExec"))
                && matches!(row.get("operator info"), Some(Value::String(info)) if info.contains("range_column=score"))
        }) {
            progress.index_range_plans += 1;
        } else {
            progress.fallback_plans += 1;
        }
    }
    for step in &case.steps {
        let snapshot = db
            .begin_read_transaction()
            .map_err(|err| error("snapshot", err))?;
        let before = observe(&case.relations, "before", |sql, params| {
            snapshot.query_sql_with_params(sql, params)
        })?;
        let epoch = db.commit_epoch().map_err(|err| error("epoch", err))?;
        let mut tx = db.begin_transaction().map_err(|err| error("begin", err))?;
        let mut rejected = false;
        for mutation in &step.mutations {
            match tx.query_sql_with_params(&mutation.sql, &mutation.parameters) {
                Ok(_) => {}
                Err(err)
                    if step
                        .expected_error
                        .is_some_and(|message| err.to_string().contains(message)) =>
                {
                    rejected = true;
                    break;
                }
                Err(err) => {
                    let mut failure = error(&format!("mutation_{}", step.name), err);
                    failure.evidence["mutation"] = mutation.json();
                    return Err(failure);
                }
            }
        }
        if step.expected_error.is_some() && !rejected {
            return Err(Failure {
                signature: "expected_rejection_missing".into(),
                evidence: json!({"expected_error": step.expected_error}),
            });
        }
        let staged = if rejected {
            before.clone()
        } else {
            observe(&case.relations, "staged", |sql, params| {
                tx.query_sql_with_params(sql, params)
            })?
        };
        if step.rollback || rejected {
            tx.rollback();
        } else {
            tx.commit().map_err(|err| error("commit", err))?;
        }
        let after = observe(&case.relations, "published", |sql, params| {
            db.query_sql_with_params(sql, params)
        })?;
        unchanged(
            &case.relations,
            "publication",
            if step.rollback || rejected {
                &before
            } else {
                &staged
            },
            &after,
        )?;
        if (step.rollback || rejected)
            && db.commit_epoch().map_err(|err| error("epoch", err))? != epoch
        {
            return Err(Failure {
                signature: "rollback_epoch".into(),
                evidence: json!({"before_epoch": epoch, "after_epoch": db.commit_epoch().ok()}),
            });
        }
        let old = observe(&case.relations, "old_snapshot", |sql, params| {
            snapshot.query_sql_with_params(sql, params)
        })?;
        unchanged(&case.relations, "snapshot_changed", &before, &old)?;
        progress.completed_steps += 1;
        *progress.operations.entry(step.name).or_default() += 1;
    }
    Ok(())
}

fn reduce(case: &Case, signature: &str) -> JsonValue {
    let mut reduced = case.clone();
    let mut attempts = 0;
    let mut index = 0;
    // Remove entire transactions, preserving the paired writes and rollback
    // boundary. A setup/execution error cannot replace a result mismatch.
    while index < reduced.steps.len() && attempts < MAX_SQL_REDUCTION_ATTEMPTS {
        let mut candidate = reduced.clone();
        candidate.steps.remove(index);
        attempts += 1;
        if run(&candidate, &mut Progress::default())
            .is_err_and(|failure| failure.signature == signature)
        {
            reduced = candidate;
        } else {
            index += 1;
        }
    }
    json!({ "attempts": attempts, "original_step_count": case.steps.len(),
        "reduced_step_count": reduced.steps.len(), "replay": reduced.replay() })
}

pub(super) fn evaluate(seed: u64, index: usize) -> JsonValue {
    let case = generator::generate(seed, index);
    let mut progress = Progress::default();
    let failure = run(&case, &mut progress).err();
    json!({
        "protocol": PROTOCOL, "success": failure.is_none(),
        "requested_steps": case.steps.len(), "completed_steps": progress.completed_steps,
        "operations": progress.operations,
        "index_range_plans": progress.index_range_plans, "fallback_plans": progress.fallback_plans,
        "failure": failure.as_ref().map(|failure| json!({
            "signature": failure.signature, "step_index": progress.completed_steps,
            "evidence": failure.evidence, "replay": case.replay(),
            "reduction": reduce(&case, &failure.signature)
        }))
    })
}

pub(crate) fn coverage(cases: &[JsonValue]) -> JsonValue {
    let mut operations = BTreeMap::<String, u64>::new();
    let (mut attempted, mut passed, mut indexed, mut fallback) = (0, 0, 0, 0);
    for report in cases.iter().map(|case| &case["sql"]["maintenance"]) {
        if report.is_null() {
            continue;
        }
        attempted += 1;
        passed += u64::from(report["success"] == true);
        indexed += report["index_range_plans"].as_u64().unwrap_or(0);
        fallback += report["fallback_plans"].as_u64().unwrap_or(0);
        if let Some(counts) = report["operations"].as_object() {
            for (operation, count) in counts {
                *operations.entry(operation.clone()).or_default() += count.as_u64().unwrap_or(0);
            }
        }
    }
    json!({ "attempted": attempted, "passed": passed, "failed": attempted - passed,
        "operations": operations, "index_range_plans": indexed, "fallback_plans": fallback })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maintenance_campaign_covers_mutations_ranges_and_snapshot_boundaries() {
        for index in 0..8 {
            let case = generator::generate(71 + index as u64, index);
            assert_eq!(case, generator::generate(71 + index as u64, index));
            let mut progress = Progress::default();
            run(&case, &mut progress).unwrap_or_else(|failure| {
                panic!(
                    "seed={} step={}: {failure:?}",
                    case.seed, progress.completed_steps
                )
            });
            assert_eq!(progress.completed_steps, case.steps.len());
            assert!(progress.index_range_plans > 0);
            for name in [
                "insert",
                "upsert",
                "update",
                "delete",
                "rollback",
                "clear",
                "repopulate",
                "overflow_rejection",
            ] {
                assert!(progress.operations.contains_key(name), "missing {name}");
            }
        }
    }

    #[test]
    fn corrupted_view_is_detected_and_reduced_without_accepting_setup_errors() {
        let mut case = generator::generate(71, 0);
        case.steps.insert(0, Step {
            name: "corrupt_view", rollback: false,
                expected_error: None,
            mutations: vec![SqlMutation::required("DROP MATERIALIZED VIEW sm_projection"),
                SqlMutation::required("CREATE MATERIALIZED VIEW sm_projection AS SELECT bucket, score, tag FROM sm_source WHERE enabled = false")],
        });
        let failure = run(&case, &mut Progress::default()).unwrap_err();
        assert_eq!(failure.signature, "staged_projection_rows");
        let reduction = reduce(&case, &failure.signature);
        assert_eq!(reduction["reduced_step_count"], 1);
        assert_eq!(reduction["replay"]["steps"][0]["name"], "corrupt_view");
        case.setup[0].sql = "SELECT * FROM absent_table".into();
        let setup = run(&case, &mut Progress::default()).unwrap_err();
        assert_ne!(setup.signature, failure.signature);
    }

    #[test]
    fn comparisons_preserve_bags_order_and_null_types() {
        let case = generator::generate(7, 0);
        let row = BTreeMap::from([("score".into(), Value::Null)]);
        let other = BTreeMap::from([("score".into(), Value::Int(0))]);
        let relation = &case.relations[0];
        assert!(compare(
            "test",
            relation,
            &[row.clone(), row.clone()],
            std::slice::from_ref(&row)
        )
        .is_err());
        assert!(compare(
            "test",
            relation,
            std::slice::from_ref(&row),
            std::slice::from_ref(&other)
        )
        .is_err());
        let ordered = case
            .relations
            .iter()
            .find(|relation| relation.name.starts_with("range_"))
            .unwrap();
        assert!(compare(
            "test",
            ordered,
            &[row.clone(), other.clone()],
            &[other, row]
        )
        .is_err());
    }
    #[test]
    fn rejection_oracle_requires_the_expected_failure() {
        let mut case = generator::generate(7, 0);
        let rejected = case
            .steps
            .iter_mut()
            .find(|step| step.expected_error.is_some())
            .unwrap();
        rejected.mutations.pop();
        let failure = run(&case, &mut Progress::default()).unwrap_err();
        assert_eq!(failure.signature, "expected_rejection_missing");
        let rejected = case
            .steps
            .iter_mut()
            .find(|step| step.expected_error.is_some())
            .unwrap();
        rejected
            .mutations
            .push(SqlMutation::required("SELECT * FROM absent_table"));
        let failure = run(&case, &mut Progress::default()).unwrap_err();
        assert!(failure
            .signature
            .starts_with("mutation_overflow_rejection_"));
    }
}

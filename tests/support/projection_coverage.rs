// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use std::num::NonZeroU64;

fn caps() -> ProjectionCoverageConfig {
    ProjectionCoverageConfig {
        feed: config(0).feed,
        source: Default::default(),
        mapping: ProjectionBaselineMappingLimits {
            calls: nz(1000),
            queries: nz(1000),
            reserved_rows: nz(4000),
            reserved_payload_bytes: nz(4 << 20),
            queries_per_call: nz(1),
        },
        comparison: SearchBaselineComparisonLimits {
            lookups: nz(1000),
            compressed_segment_bytes: NonZeroU64::new(65536).unwrap(),
            expanded_segment_bytes: NonZeroU64::new(65536).unwrap(),
            total_compressed_bytes: NonZeroU64::new(64 << 20).unwrap(),
            total_expanded_bytes: NonZeroU64::new(64 << 20).unwrap(),
        },
    }
}
fn full_e(
    ctx: &mut ProjectionBaselineMappingContext<'_, '_>,
    input: ProjectionFullSourceItem<'_>,
) -> Result<Option<SearchDocument>> {
    match input {
        ProjectionFullSourceItem::GraphNodeCandidate { node_id } => {
            let rows = ctx.query_with_params(
                "MATCH (n:Memory) WHERE id(n)=$id RETURN n.id AS id,n.value AS value",
                &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
            )?;
            let Some(row) = rows.output.rows.first() else {
                return Ok(None);
            };
            let Value::String(id) = &row["id"] else {
                panic!("text ID");
            };
            let Value::Int(value) = row["value"] else {
                panic!("integer value");
            };
            Ok(Some(doc(&format!("memory:{id}"), &value.to_string())))
        }
        ProjectionFullSourceItem::RelationalKey {
            table: "records",
            key,
        } => {
            let RelationalValue::Text(id) = &key.0[0] else {
                panic!("text PK");
            };
            let rows = ctx.query_sql_with_params(
                "SELECT body FROM records WHERE id=$1",
                &[Value::String(id.clone())],
            )?;
            let Value::String(body) = &rows.rows[0]["body"] else {
                panic!("text body");
            };
            Ok(Some(doc(&format!("record:{id}"), body)))
        }
        ProjectionFullSourceItem::RelationalKey { table, .. }
        | ProjectionFullSourceItem::RelationalTable { table }
            if !matches!(table, "records" | "hawdb_schema_migrations") =>
        {
            Err(HawDBError::Execution(format!("unclassified table {table}")))
        }
        _ => Ok(None),
    }
}
fn delta_e(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    match input {
        ProjectionMappingInput::GraphNode { node_id, .. } => {
            let rows = ctx.query_with_params(
                "MATCH (n:Memory) WHERE id(n)=$id RETURN n.id AS id,n.value AS value",
                &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
            )?;
            if let Some(row) = rows.output.rows.first() {
                let Value::String(id) = &row["id"] else {
                    panic!("text ID");
                };
                let Value::Int(value) = row["value"] else {
                    panic!("integer value");
                };
                ctx.upsert(doc(&format!("memory:{id}"), &value.to_string()))?;
            }
        }
        ProjectionMappingInput::RelationalKey {
            table: "records",
            key,
            ..
        } => {
            let RelationalValue::Text(id) = &key.0[0] else {
                panic!("text PK");
            };
            let rows = ctx.query_sql_with_params(
                "SELECT body FROM records WHERE id=$1",
                &[Value::String(id.clone())],
            )?;
            let document_id = format!("record:{id}");
            if let Some(row) = rows.rows.first() {
                let Value::String(body) = &row["body"] else {
                    panic!("text body");
                };
                ctx.upsert(doc(&document_id, body))?;
            } else {
                ctx.delete(&document_id)?;
            }
        }
        ProjectionMappingInput::DeleteDocument { document_id, .. } => ctx.delete(document_id)?,
        _ => {}
    }
    Ok(ProjectionMappingStatus::Done)
}
fn one() -> ProjectionFeedLimits {
    ProjectionFeedLimits {
        items: nz(1),
        ..feed()
    }
}
fn sort() -> SearchMutationSortStepLimits {
    SearchMutationSortStepLimits {
        work: nz(16),
        ..Default::default()
    }
}
fn stage<'a>(
    db: &Database,
    reader: &'a SearchOutOfCoreReader,
    c: u64,
) -> ProjectionStagingSession<'a> {
    let mut s = db
        .begin_projection_staging_session(reader, config(c), delta_e, admission())
        .unwrap();
    for _ in 0..10000 {
        if s.advance(one(), sort()).unwrap() == ProjectionStagingSessionStep::Complete {
            return s;
        }
    }
    panic!("staging did not finish");
}
fn covered(session: ProjectionStagingSession<'_>) -> ProjectionCoveredUpdate {
    let mut s = session.verify_complete_source(caps(), full_e).unwrap();
    for _ in 0..10000 {
        if s.advance(one(), sort()).unwrap() == ProjectionCoverageStep::Complete {
            return s.finish().unwrap();
        }
    }
    panic!("coverage did not finish");
}
fn oracle(db: &mut Database, root: &Root, epoch: u64) -> SearchOutOfCoreReader {
    let path = root.0.join("oracle");
    SearchOutOfCoreGenerationWriter::create(&path, Default::default())
        .unwrap()
        .finish()
        .unwrap();
    let base = SearchOutOfCoreReader::open(&path).unwrap();
    let c = config(epoch);
    let mut rebuild = db
        .begin_projection_full_rebuild(
            &base,
            ProjectionFullRebuildConfig {
                reads: c.reads,
                feed: c.feed,
                source: caps().source,
                mapping: caps().mapping,
                sorting: c.sorting,
                build: c.build,
                task: c.task,
            },
            full_e,
            admission(),
        )
        .unwrap();
    for _ in 0..10000 {
        if rebuild.advance(one(), sort()).unwrap() == ProjectionFullRebuildStep::Complete {
            break;
        }
    }
    db.publish_projection_full_rebuild(
        rebuild.finish().unwrap(),
        ProjectionSourceDurability::RequireKnownDurable,
    )
    .unwrap();
    SearchOutOfCoreReader::open(path).unwrap()
}

#[test]
fn one_operation_catch_up_matches_full_rebuild_after_update_delete_and_aba() {
    for cold in [false, true] {
        for aba in [false, true] {
            let (root, mut db, reader, c) = setup(cold);
            db.query("MATCH (n:Memory) DETACH DELETE n").unwrap();
            db.query_sql("DELETE FROM records WHERE id='a'").unwrap();
            if aba {
                db.query("CREATE (:Memory {id:'a',value:3})").unwrap();
                db.query_sql("INSERT INTO records (id,body) VALUES ('a','final')")
                    .unwrap();
            }
            db.query_sql("INSERT INTO records (id,body) VALUES ('z','kept')")
                .unwrap();
            let e = db.commit_epoch().unwrap();
            let session = stage(&db, &reader, c);
            unchanged(&root, &reader);
            let candidate = covered(session);
            assert_eq!(candidate.source_epoch(), e);
            unchanged(&root, &reader);
            db.publish_projection_covered_update(
                candidate,
                ProjectionSourceDurability::RequireKnownDurable,
            )
            .unwrap();
            let expected = oracle(&mut db, &root, e);
            // A lost caller response does not roll back the selected generation.
            drop(reader);
            drop(db);
            let reopened = Database::open(root.0.join("graph")).unwrap();
            assert_eq!(reopened.commit_epoch().unwrap(), e);
            let actual = SearchOutOfCoreReader::open(root.0.join("search")).unwrap();
            assert_eq!(actual.source_graph_commit_epoch(), Some(e));
            assert_eq!(actual.document_count(), expected.document_count());
            let ids = if aba {
                vec!["memory:a".into(), "record:a".into(), "record:z".into()]
            } else {
                vec!["record:z".into()]
            };
            assert_eq!(
                actual.hydrate_documents(&ids).unwrap().documents,
                expected.hydrate_documents(&ids).unwrap().documents
            );
        }
    }
}

#[test]
fn stale_source_and_wrong_instance_cannot_activate_a_covered_candidate() {
    for wrong_instance in [false, true] {
        let (root, mut db, reader, c) = setup(false);
        let session = stage(&db, &reader, c);
        let e = session.source_epoch();
        if !wrong_instance {
            // Full-source verification must retain E even if it starts later.
            db.query_sql("UPDATE records SET body='after-E' WHERE id='a'")
                .unwrap();
        }
        let candidate = covered(session);
        assert_eq!(candidate.source_epoch(), e);
        if wrong_instance {
            let mut other = Database::new();
            assert!(other
                .publish_projection_covered_update(
                    candidate,
                    ProjectionSourceDurability::RequireKnownDurable
                )
                .is_err());
        } else {
            assert!(db
                .publish_projection_covered_update(
                    candidate,
                    ProjectionSourceDurability::RequireKnownDurable
                )
                .is_err());
        }
        unchanged(&root, &reader);
    }
}

fn omit_deletes(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    state: &[u8],
) -> Result<ProjectionMappingStatus> {
    if matches!(input, ProjectionMappingInput::DeleteDocument { .. }) {
        return Ok(ProjectionMappingStatus::Done);
    }
    delta_e(ctx, input, state)
}
#[test]
fn missing_delete_is_rejected_by_complete_set_comparison() {
    let (root, mut db, reader, c) = setup(false);
    db.query("MATCH (n:Memory) DETACH DELETE n").unwrap();
    let mut s = db
        .begin_projection_staging_session(&reader, config(c), omit_deletes, admission())
        .unwrap();
    complete(&mut s);
    let mut verify = s.verify_complete_source(caps(), full_e).unwrap();
    let mut failed = false;
    for _ in 0..10000 {
        if verify.advance(one(), sort()).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    assert!(matches!(verify.finish(), Err(ProjectionFeedError::Failed)));
    unchanged(&root, &reader);
}

#[test]
fn repeated_keys_and_rename_in_one_transaction_match_full_rebuild() {
    let (root, mut db, reader, c) = setup(false);
    let mut transaction = db.begin_transaction().unwrap();
    for value in 3..27 {
        transaction
            .query(&format!("MATCH (n:Memory) SET n.value={value}"))
            .unwrap();
    }
    transaction
        .query("MATCH (n:Memory) SET n.id='renamed'")
        .unwrap();
    transaction.commit().unwrap();
    let e = db.commit_epoch().unwrap();
    let candidate = covered(stage(&db, &reader, c));
    db.publish_projection_covered_update(
        candidate,
        ProjectionSourceDurability::RequireKnownDurable,
    )
    .unwrap();
    let expected = oracle(&mut db, &root, e);
    let actual = SearchOutOfCoreReader::open(root.0.join("search")).unwrap();
    let ids = ["memory:renamed".into(), "record:a".into()];
    assert_eq!(actual.document_count(), expected.document_count());
    assert_eq!(
        actual.hydrate_documents(&ids).unwrap().documents,
        expected.hydrate_documents(&ids).unwrap().documents
    );
    // Empty C..E still traverses End and proves unchanged base content.
    let empty = covered(stage(&db, &actual, e));
    assert_eq!(empty.source_epoch(), e);
    db.publish_projection_covered_update(empty, ProjectionSourceDurability::RequireKnownDurable)
        .unwrap();
}

fn wrong_body(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    state: &[u8],
) -> Result<ProjectionMappingStatus> {
    if matches!(input, ProjectionMappingInput::GraphNode { .. }) {
        ctx.upsert(doc("memory:a", "wrong"))?;
        return Ok(ProjectionMappingStatus::Done);
    }
    delta_e(ctx, input, state)
}
#[test]
fn wrong_body_duplicate_source_and_budget_exhaustion_cannot_mint_coverage() {
    for case in ["body", "duplicate", "lookups", "calls", "source_bytes"] {
        let (root, mut db, reader, c) = setup(false);
        if case == "duplicate" {
            db.query("CREATE (:Memory {id:'a',value:2})").unwrap();
        }
        let mapper = if case == "body" { wrong_body } else { delta_e };
        let mut s = db
            .begin_projection_staging_session(&reader, config(c), mapper, admission())
            .unwrap();
        complete(&mut s);
        let mut cfg = caps();
        match case {
            "lookups" => cfg.comparison.lookups = nz(1),
            "calls" => cfg.mapping.calls = nz(1),
            "source_bytes" => cfg.feed.bytes = nz(1),
            _ => {}
        }
        let mut verify = s.verify_complete_source(cfg, full_e).unwrap();
        let mut failed = false;
        for _ in 0..10000 {
            if verify.advance(one(), sort()).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "{case}");
        assert!(verify.finish().is_err(), "{case}");
        unchanged(&root, &reader);
    }
}

#[test]
fn covered_candidate_retains_the_original_selector_fence() {
    let (root, mut db, reader, c) = setup(false);
    let first = covered(stage(&db, &reader, c));
    let second = covered(stage(&db, &reader, c));
    db.publish_projection_covered_update(first, ProjectionSourceDurability::RequireKnownDurable)
        .unwrap();
    let selected = SearchOutOfCoreReader::open(root.0.join("search"))
        .unwrap()
        .generation();
    assert!(db
        .publish_projection_covered_update(second, ProjectionSourceDurability::RequireKnownDurable)
        .is_err());
    assert_eq!(
        SearchOutOfCoreReader::open(root.0.join("search"))
            .unwrap()
            .generation(),
        selected
    );
}

#[test]
fn cancellation_at_each_coverage_phase_preserves_base_and_allows_reopen_retry() {
    for phase in [
        ProjectionCoverageStep::Input,
        ProjectionCoverageStep::Ordering,
        ProjectionCoverageStep::Comparing,
        ProjectionCoverageStep::Complete,
    ] {
        let (root, db, reader, c) = setup(false);
        let cfg = config(c);
        let cancel = cfg.task.cancellation().clone();
        let mut s = db
            .begin_projection_staging_session(&reader, cfg, delta_e, admission())
            .unwrap();
        complete(&mut s);
        let mut verify = s.verify_complete_source(caps(), full_e).unwrap();
        let mut reached = false;
        for _ in 0..10000 {
            if verify.advance(one(), sort()).unwrap() == phase {
                reached = true;
                break;
            }
        }
        assert!(reached, "coverage did not reach {phase:?}");
        cancel.cancel();
        assert!(verify.advance(one(), sort()).is_err());
        assert!(verify.finish().is_err());
        unchanged(&root, &reader);
        drop(db);
        let mut db = Database::open(root.0.join("graph")).unwrap();
        let candidate = covered(stage(&db, &reader, c));
        // Replayed WAL does not establish an observed durability barrier.
        db.publish_projection_covered_update(candidate, ProjectionSourceDurability::Synchronize)
            .unwrap();
    }
}

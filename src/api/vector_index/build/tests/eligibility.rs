// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::{QueryRequest, QueryStreamOptions, VectorIndexLanguageBudget, VectorIndexSqlRequest};

fn budget() -> VectorIndexLanguageBudget {
    VectorIndexLanguageBudget {
        index_working_bytes: NonZeroUsize::new(32 << 20).unwrap(),
        index_result_bytes: NonZeroUsize::new(1 << 20).unwrap(),
    }
}

#[test]
fn vector_index_exact_eligibility_finds_neighbors_after_an_ineligible_prefix() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql(
        "CREATE TABLE vectors (id BIGINT PRIMARY KEY, active BOOLEAN, embedding VECTOR(2))",
    )
    .unwrap();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    for (id, active, coordinates) in [
        (1, false, Some([1.0, 0.0])),
        (2, false, Some([0.9, 0.1])),
        (3, true, Some([0.0, 1.0])),
        (4, true, Some([-1.0, 0.0])),
        (5, true, None),
    ] {
        let vector = coordinates
            .map(|value| Value::Vector(Vector::new(&value).unwrap()))
            .unwrap_or(Value::Null);
        db.query_sql_with_params(
            "INSERT INTO vectors (id, active, embedding) VALUES ($1, $2, $3)",
            &[Value::Int(id), Value::Bool(active), vector.clone()],
        )
        .unwrap();
        db.query_with_params(
            "CREATE (:Document {id: $id, active: $active, embedding: $embedding})",
            &BTreeMap::from([
                ("id".into(), Value::Int(id)),
                ("active".into(), Value::Bool(active)),
                ("embedding".into(), vector),
            ]),
        )
        .unwrap();
    }
    let sql = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("sql"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let graph = db
        .create_vector_index(
            "nodes",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("graph"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let sql_read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&sql, &permit)
        .unwrap();
    let mut graph_read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&graph, &permit)
        .unwrap();
    let embedding = Value::Vector(Vector::new(&[1.0, 0.0]).unwrap());
    let parameters = BTreeMap::from([("q".into(), embedding.clone())]);
    let exact_sql = "SELECT v.id, distance FROM vector_index_search('rows', $1, mode => 'exact', top_k => 2, max_work => 5) AS v WHERE v.active = true ORDER BY distance, v.id";
    let exact_graph = "MATCH (d:Document) WHERE d.active = true CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := 2, maxWork := 100) YIELD node AS n, distance RETURN n.id AS id, distance ORDER BY distance, id";
    let mut sql_rows = Vec::new();
    let sql_report = sql_read
        .query_sql_streaming(
            &permit,
            budget(),
            VectorIndexSqlRequest {
                sql: exact_sql,
                parameters: std::slice::from_ref(&embedding),
                output_limits: QueryStreamOptions::default(),
                task: Default::default(),
            },
            |row| {
                sql_rows.push(row);
                Ok(())
            },
        )
        .unwrap();
    let mut graph_rows = Vec::new();
    let graph_report = graph_read
        .query_cypher_streaming(
            &permit,
            budget(),
            QueryRequest::new(exact_graph).with_params(&parameters),
            |row| {
                graph_rows.push(row);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(sql_rows, graph_rows);
    assert_eq!(
        sql_rows
            .iter()
            .map(|row| row["id"].clone())
            .collect::<Vec<_>>(),
        [Value::Int(3), Value::Int(4)]
    );
    assert_eq!(
        sql_rows
            .iter()
            .map(|row| row["distance"].clone())
            .collect::<Vec<_>>(),
        [Value::Float(1.0), Value::Float(2.0)]
    );
    for (report, input_rows) in [(sql_report.index(), 5), (graph_report.index().unwrap(), 3)] {
        assert_eq!(report.mode, hawdb_core::VectorIndexQueryMode::Exact);
        assert_eq!(report.backend, "exact");
        assert_eq!(report.candidate_limit, 0);
        let eligibility = report.eligibility.unwrap();
        assert!(eligibility.complete);
        assert_eq!(eligibility.input_rows, input_rows);
        assert_eq!(eligibility.eligible_vectors, 2);
        assert_eq!(eligibility.scored_vectors, 2);
    }
    let window_sql = "SELECT v.id FROM vector_index_search('rows', $1, candidates => 8, top_k => 2) AS v WHERE v.active = true";
    let mut window_rows = 0;
    sql_read
        .query_sql_streaming(
            &permit,
            budget(),
            VectorIndexSqlRequest {
                sql: window_sql,
                parameters: std::slice::from_ref(&embedding),
                output_limits: QueryStreamOptions::default(),
                task: Default::default(),
            },
            |_| {
                window_rows += 1;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(window_rows, 0, "legacy window must not refill");
    for rejected_sql in [
        exact_sql.replace("max_work => 5", "max_work => 4"),
        exact_sql.replace("v.active = true", "distance < 1.5"),
        exact_sql.replace("v.active = true", "v.absent = true"),
    ] {
        let result = sql_read.query_sql_streaming(
            &permit,
            budget(),
            VectorIndexSqlRequest {
                sql: &rejected_sql,
                parameters: std::slice::from_ref(&embedding),
                output_limits: QueryStreamOptions::default(),
                task: Default::default(),
            },
            |_| panic!("invalid domain must not deliver rows"),
        );
        assert!(result.is_err(), "{rejected_sql}");
    }
    let rejected_graph = exact_graph.replace("maxWork := 100", "maxWork := 1");
    assert!(graph_read
        .query_cypher_streaming(
            &permit,
            budget(),
            QueryRequest::new(&rejected_graph).with_params(&parameters),
            |_| panic!("partial domain must not deliver rows")
        )
        .is_err());
    // A failed execution leaves both retained domains reusable.
    graph_read
        .query_cypher_streaming(
            &permit,
            budget(),
            QueryRequest::new(exact_graph).with_params(&parameters),
            |_| Ok(()),
        )
        .unwrap();
}

#[test]
fn vector_index_exact_graph_preserves_limit_before_identity_deduplication() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    let embedding = Value::Vector(Vector::new(&[1.0, 0.0]).unwrap());
    for id in [1, 2] {
        db.query_with_params(
            "CREATE (:Document {id: $id, embedding: $q})",
            &BTreeMap::from([
                ("id".into(), Value::Int(id)),
                ("q".into(), embedding.clone()),
            ]),
        )
        .unwrap();
    }
    for id in [10, 20] {
        db.query_with_params(
            "CREATE (:Origin {id: $id})",
            &BTreeMap::from([("id".into(), Value::Int(id))]),
        )
        .unwrap();
    }
    for (origin, document) in [(10, 1), (20, 1), (20, 2)] {
        db.query_with_params("MATCH (o:Origin {id: $origin}), (d:Document {id: $document}) CREATE (o)-[:RELATED]->(d)", &BTreeMap::from([("origin".into(), Value::Int(origin)), ("document".into(), Value::Int(document))])).unwrap();
    }
    let consumer = db
        .create_vector_index(
            "nodes",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("graph"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let mut read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    let parameters = BTreeMap::from([("q".into(), embedding)]);
    for (limit, ids, inputs) in [
        ("LIMIT 2", vec![Value::Int(1)], 2),
        ("", vec![Value::Int(1), Value::Int(2)], 3),
    ] {
        let query = format!("MATCH (o:Origin)-[:RELATED]->(d:Document) WITH d ORDER BY d.id {limit} CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := 2, maxWork := 100) YIELD node AS n, distance RETURN n.id AS id, distance ORDER BY id");
        let mut rows = Vec::new();
        let report = read
            .query_cypher_streaming(
                &permit,
                budget(),
                QueryRequest::new(&query).with_params(&parameters),
                |row| {
                    rows.push(row["id"].clone());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(rows, ids);
        let eligibility = report.index().unwrap().eligibility.unwrap();
        assert_eq!(eligibility.input_rows, inputs);
        assert_eq!(eligibility.eligible_vectors, rows.len());
    }
}

#[test]
fn vector_index_exact_bounds_rejected_source_work_and_retries_after_reopen() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    let q = Value::Vector(Vector::new(&[1.0, 0.0]).unwrap());
    db.query("CREATE (:Origin {id: 100})").unwrap();
    for id in 1..=5 {
        db.query_with_params(
            "CREATE (:Document {id: $id, active: false, embedding: $q})",
            &BTreeMap::from([("id".into(), Value::Int(id)), ("q".into(), q.clone())]),
        )
        .unwrap();
        db.query_with_params(
            "MATCH (o:Origin), (d:Document {id: $id}) CREATE (o)-[:RELATED {active: false}]->(d)",
            &BTreeMap::from([("id".into(), Value::Int(id))]),
        )
        .unwrap();
    }
    db.query("CREATE INDEX ON :Document(active)").unwrap();
    let origin = db
        .query("MATCH (o:Origin) RETURN id(o) AS node_id")
        .unwrap()
        .rows[0]["node_id"]
        .clone();
    let consumer = db
        .create_vector_index(
            "nodes",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("graph"),
            registration(),
            Default::default(),
        )
        .unwrap();
    drop(consumer);
    let parameters = BTreeMap::from([("q".into(), q), ("origin".into(), origin)]);
    for reopened in [false, true] {
        if reopened {
            db.checkpoint().unwrap();
            drop(db);
            db = fixture.database();
        }
        let consumer = db
            .open_search_projection_consumer(
                &SearchProjectionConsumerId::new("nodes").unwrap(),
                fixture.0.join("graph"),
            )
            .unwrap();
        let permit = db
            .search_projection_consumer_read_permit(
                RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
            )
            .unwrap();
        let mut read = db
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit)
            .unwrap();
        for prefix in [
            "MATCH (d:Document) WHERE d.active = true",
            "MATCH (o)-[r:RELATED]->(d:Document) WHERE id(o) = $origin AND r.active = true",
        ] {
            let query = format!("{prefix} CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := 2, maxWork := 2) YIELD node AS n, distance RETURN n.id AS id");
            let error = read
                .query_cypher_streaming(
                    &permit,
                    budget(),
                    QueryRequest::new(&query).with_params(&parameters),
                    |_| panic!("work refusal must not deliver rows"),
                )
                .unwrap_err();
            assert!(
                matches!(error, crate::VectorIndexReadError::Query(HawDBError::ResourceBudgetExceeded(ref error)) if error.resource == hawdb_core::ResourceBudgetKind::WorkUnits),
                "{error:?}"
            );
            let retry = query.replace("maxWork := 2", "maxWork := 1000");
            let report = read
                .query_cypher_streaming(
                    &permit,
                    budget(),
                    QueryRequest::new(&retry).with_params(&parameters),
                    |_| panic!("all source records are ineligible"),
                )
                .unwrap();
            let eligible = report.index().unwrap().eligibility.unwrap();
            assert_eq!(eligible.input_rows, 0);
            assert_eq!(eligible.eligible_vectors, 0);
            assert!(eligible.work_units > 2);
            assert_eq!(eligible.source_strategy, "bounded_canonical");
            assert!(eligible.complete);
            let task = hawdb_core::RuntimeTaskContext::default();
            task.cancellation().cancel();
            assert!(read
                .query_cypher_streaming(
                    &permit,
                    budget(),
                    QueryRequest::new(&retry)
                        .with_params(&parameters)
                        .with_task_context(&task),
                    |_| panic!("cancelled input must not deliver rows")
                )
                .is_err());
        }
    }
}

#[cfg(feature = "acl")]
#[test]
fn vector_index_exact_visibility_precedes_top_k() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    for (id, scope, vector) in [(1, "in", [1.0, 0.0]), (2, "in", [0.0, 1.0])] {
        db.query_with_params(
            "CREATE (:Document {id: $id, scope: $scope, embedding: $q})",
            &BTreeMap::from([
                ("id".into(), Value::Int(id)),
                ("scope".into(), Value::String(scope.into())),
                ("q".into(), Value::Vector(Vector::new(&vector).unwrap())),
            ]),
        )
        .unwrap();
    }
    db.query("CREATE (:Origin {id: 1, scope: 'out'})").unwrap();
    db.query("CREATE (:Origin {id: 2, scope: 'in'})").unwrap();
    for id in [1, 2] {
        db.query_with_params(
            "MATCH (o:Origin {id: $id}), (d:Document {id: $id}) CREATE (o)-[:LINK]->(d)",
            &BTreeMap::from([("id".into(), Value::Int(id))]),
        )
        .unwrap();
    }
    let consumer = db
        .create_vector_index(
            "nodes",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("graph"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let mut read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    let parameters =
        BTreeMap::from([("q".into(), Value::Vector(Vector::new(&[1.0, 0.0]).unwrap()))]);
    let access = crate::QueryAccessControlContext::visibility_scope(42, "scope", "in");
    let query = "MATCH (o:Origin)-[:LINK]->(d:Document) CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := 1, maxWork := 100) YIELD node AS n, distance RETURN n.id AS id, distance";
    let mut rows = Vec::new();
    let report = read
        .query_cypher_streaming(
            &permit,
            budget(),
            QueryRequest::new(query)
                .with_params(&parameters)
                .with_access_control(&access),
            |row| {
                rows.push(row);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::Int(2));
    assert_eq!(rows[0]["distance"], Value::Float(1.0));
    assert_eq!(
        report
            .index()
            .unwrap()
            .eligibility
            .unwrap()
            .eligible_vectors,
        1
    );
}

#[test]
fn vector_index_exact_refuses_domains_from_a_different_native_binding() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    let consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("sql"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    let vector = Vector::new(&[1.0, 0.0]).unwrap();
    let memory = hawdb_executor::ExecutionMemoryConfig::default();
    let ledger = hawdb_executor::QueryMemoryLedger::for_query(&memory, None).unwrap();
    let account = ledger.account(
        hawdb_executor::QueryMemoryClass::BlockingState,
        "eligible binding test",
        memory.blocking_operator_bytes,
    );
    let identity = read.pin.vector_index_read_identity();
    for changed in [
        hawdb_core::VectorIndexReadIdentity {
            source_epoch: identity.source_epoch + 1,
            ..identity
        },
        hawdb_core::VectorIndexReadIdentity {
            generation: NonZeroU64::new(identity.generation.get() + 1).unwrap(),
            ..identity
        },
        hawdb_core::VectorIndexReadIdentity {
            target_incarnation: NonZeroU64::new(identity.target_incarnation.get() + 1).unwrap(),
            ..identity
        },
        hawdb_core::VectorIndexReadIdentity {
            branch: hawdb_core::generate_uuidv7().unwrap(),
            ..identity
        },
        hawdb_core::VectorIndexReadIdentity {
            database: hawdb_core::generate_uuidv7().unwrap(),
            ..identity
        },
    ] {
        let mut domain = hawdb_executor::vector_eligibility::VectorEligibleDomain::new(
            &read.definition,
            changed,
            vector.as_ref(),
            0,
            &account,
            None,
        )
        .unwrap();
        domain.finish().unwrap();
        let request = hawdb_search::VectorIndexQuery {
            descriptor: &read.definition,
            embedding: &[1.0, 0.0],
            candidate_limit: NonZeroUsize::MIN,
            top_k: NonZeroUsize::MIN,
            mode: hawdb_core::VectorIndexQueryMode::Exact,
            eligible: Some(&domain),
        };
        let error = read
            .pin
            .query_vector_index(&permit, Default::default(), request)
            .unwrap_err();
        assert!(
            error.to_string().contains("another source or publication"),
            "{error}"
        );
    }
}

#[test]
fn vector_index_exact_preserves_ordered_limit_and_rebinds_parameters() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    let vector = Value::Vector(Vector::new(&[1.0, 0.0]).unwrap());
    for rank in [3, 2, 1] {
        db.query_with_params(
            "CREATE (:Document {rank: $rank, embedding: $q})",
            &BTreeMap::from([
                ("rank".into(), Value::Int(rank)),
                ("q".into(), vector.clone()),
            ]),
        )
        .unwrap();
    }
    db.query("CREATE INDEX ON :Document(rank)").unwrap();
    let consumer = db
        .create_vector_index(
            "nodes",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("graph"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let mut read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    let query = "MATCH (d:Document) WHERE d.rank >= $minimum WITH d ORDER BY d.rank LIMIT $limit CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := $top, maxWork := $work) YIELD node AS n, distance RETURN n.rank AS rank ORDER BY rank";
    for (minimum, limit, top, expected) in [
        (1, 1, 1, vec![1]),
        (2, 2, 2, vec![2, 3]),
        (1, 2, 2, vec![1, 2]),
    ] {
        let mut parameters = BTreeMap::from([
            ("q".into(), vector.clone()),
            ("minimum".into(), Value::Int(minimum)),
            ("limit".into(), Value::Int(limit)),
            ("top".into(), Value::Int(top)),
            ("work".into(), Value::Int(1000)),
        ]);
        let mut rows = Vec::new();
        read.query_cypher_streaming(
            &permit,
            budget(),
            QueryRequest::new(query).with_params(&parameters),
            |row| {
                rows.push(row["rank"].clone());
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            rows,
            expected.into_iter().map(Value::Int).collect::<Vec<_>>()
        );
        parameters.insert("work".into(), Value::Int(0));
        assert!(read
            .query_cypher_streaming(
                &permit,
                budget(),
                QueryRequest::new(query).with_params(&parameters),
                |_| panic!("work refusal delivered output")
            )
            .is_err());
        parameters.insert("work".into(), Value::Int(1000));
        let invalid_scope = query.replace("n.rank AS rank ORDER BY rank", "d.rank AS rank");
        assert!(read
            .query_cypher_streaming(
                &permit,
                budget(),
                QueryRequest::new(&invalid_scope).with_params(&parameters),
                |_| panic!("upstream binding escaped CALL")
            )
            .is_err());
    }
    for coordinates in [&[1.0_f32, 0.0, 0.0][..], &[0.0, 0.0][..]] {
        let parameters =
            BTreeMap::from([("q".into(), Value::Vector(Vector::new(coordinates).unwrap()))]);
        let empty = "MATCH (d:Document) WHERE d.rank < 0 CALL vector_index_search('nodes', $q, mode := 'exact', eligible := d, topK := 1, maxWork := 100) YIELD node RETURN node";
        assert!(read
            .query_cypher_streaming(
                &permit,
                budget(),
                QueryRequest::new(empty).with_params(&parameters),
                |_| panic!("invalid vector delivered output")
            )
            .is_err());
    }
}

#[test]
fn vector_index_exact_sql_hands_one_memory_ledger_to_output() {
    use hawdb_executor::{QueryMemoryClass, QueryMemoryLedger};
    use hawdb_relational::query::vector_index::{
        collect_vector_eligible_domain, execute_vector_index_select, VectorIndexCandidates,
        VectorIndexInput, VectorIndexSqlExecution,
    };
    // Canonical hydration needs a default 1 MiB row page plus live query state.
    const MEMORY_BYTES: usize = 4 << 20;
    struct Candidates(Vec<(String, f64)>);
    impl VectorIndexCandidates for Candidates {
        fn row_count(&self) -> usize {
            self.0.len()
        }
        fn row(&self, ordinal: usize) -> Result<(&str, f64)> {
            Ok((&self.0[ordinal].0, self.0[ordinal].1))
        }
    }
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    let embedding = Vector::new(&[1.0, 0.0]).unwrap();
    db.query_sql_with_params(
        "INSERT INTO vectors (id, embedding) VALUES (1, $1)",
        &[Value::Vector(embedding.clone())],
    )
    .unwrap();
    let consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("sql"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(128 << 20, 4 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    let prepared = hawdb_sql::prepare_postgres_sql("SELECT id FROM vector_index_search('rows', $1, mode => 'exact', top_k => 1, max_work => 10) WHERE id = 1").unwrap();
    let hawdb_sql::SqlStatement::Select(select) = &prepared.statement else {
        panic!("expected SELECT")
    };
    let ledger = QueryMemoryLedger::new(NonZeroUsize::new(MEMORY_BYTES).unwrap());
    let execution = || VectorIndexSqlExecution {
        state: read.source.store.relational_state(),
        row_mode: hawdb_relational::row_runtime::RelationalRowReadMode::Store(&read.source.store),
        resources: crate::api::relational_query_resource_context(
            &read.source.config,
            Some(10),
            Some(MEMORY_BYTES),
            None,
        ),
        memory_ledger: &ledger,
    };
    let parameters = [Value::Vector(embedding.clone())];
    let (domain, _) = collect_vector_eligible_domain(
        select,
        &parameters,
        &read.definition,
        read.pin.vector_index_read_identity(),
        embedding.as_ref(),
        10,
        execution(),
    )
    .unwrap();
    let candidates = Candidates(
        domain
            .distances()
            .map(|(id, distance)| (id.to_owned(), distance))
            .collect(),
    );
    let retained = ledger.snapshot().used_bytes;
    assert!(retained > 0);
    let output = ledger.account(
        QueryMemoryClass::BlockingState,
        "output phase oracle",
        NonZeroUsize::new(MEMORY_BYTES).unwrap(),
    );
    assert!(
        output.reserve(MEMORY_BYTES).is_err(),
        "a second phase cannot spend an independent full allowance"
    );
    drop(domain);
    assert_eq!(ledger.snapshot().used_bytes, 0);
    drop(output.reserve(MEMORY_BYTES).unwrap());
    let rows = execute_vector_index_select(
        select,
        &parameters,
        VectorIndexInput {
            table: "vectors",
            candidates: &candidates,
        },
        execution(),
    )
    .unwrap();
    assert_eq!(rows.row_count(), 1);
    assert!(ledger.snapshot().used_bytes > 0);
    drop(rows);
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use hawdb_core::Vector;
use hawdb_qos::RuntimeWorkRequest;
use std::num::NonZeroU64;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "hawdb-vector-source-{}",
            hawdb_core::generate_uuidv7().unwrap()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn database(&self) -> Database {
        Database::open(self.0.join("database")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn registration() -> SearchProjectionConsumerOptions {
    SearchProjectionConsumerOptions::new(NonZeroU64::new(100).unwrap())
}

fn sql_field() -> VectorIndexField {
    VectorIndexField::SqlColumn {
        table: "vectors".into(),
        column: "embedding".into(),
    }
}

fn distances(db: &mut Database, consumer: &SearchProjectionConsumer) -> Vec<f64> {
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let read = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(consumer, &permit)
        .unwrap();
    let output = read
        .query_candidates(
            &permit,
            Default::default(),
            &[1.0, 0.0],
            NonZeroUsize::new(8).unwrap(),
            NonZeroUsize::new(8).unwrap(),
        )
        .unwrap();
    output.hits().iter().map(|hit| hit.distance).collect()
}

#[test]
fn vector_index_builds_both_canonical_source_families_and_reopens() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (tenant TEXT, id BIGINT, embedding VECTOR(2), PRIMARY KEY (tenant, id))").unwrap();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    for (id, coordinates) in [(1, [1.0, 0.0]), (2, [0.0, 1.0]), (3, [-1.0, 0.0])] {
        let vector = Value::Vector(Vector::new(&coordinates).unwrap());
        db.query_sql_with_params(
            "INSERT INTO vectors (tenant, id, embedding) VALUES ($1, $2, $3)",
            &[Value::String("a\0b".into()), Value::Int(id), vector.clone()],
        )
        .unwrap();
        db.query_with_params(
            "CREATE (:Document {id: $id, embedding: $embedding})",
            &BTreeMap::from([("id".into(), Value::Int(id)), ("embedding".into(), vector)]),
        )
        .unwrap();
    }
    db.query_sql("INSERT INTO vectors (tenant, id, embedding) VALUES ('null', 4, NULL)")
        .unwrap();
    db.query("CREATE (:Document {id: 4})").unwrap();
    db.query("CREATE (:Other {id: 5})").unwrap();
    let targets = [
        ("sql", sql_field()),
        (
            "graph",
            VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
        ),
    ];
    for (name, field) in &targets {
        let consumer = db
            .create_vector_index(
                name,
                field,
                fixture.0.join(name),
                registration(),
                Default::default(),
            )
            .unwrap();
        assert_eq!(consumer.search_index().unwrap().document_count(), 3);
        assert_eq!(distances(&mut db, &consumer), [0.0, 1.0, 2.0]);
        assert_eq!(
            consumer
                .search_index()
                .unwrap()
                .vector_index_descriptor()
                .unwrap()
                .dimension()
                .get(),
            2
        );
    }
    drop(db);
    let mut db = fixture.database();
    for (name, _) in &targets {
        let consumer = db
            .open_search_projection_consumer(
                &SearchProjectionConsumerId::new(*name).unwrap(),
                fixture.0.join(name),
            )
            .unwrap();
        assert_eq!(distances(&mut db, &consumer), [0.0, 1.0, 2.0]);
    }
}

#[test]
fn vector_index_build_refuses_incomplete_input_and_invalid_nonnull_vectors() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    for id in 0..3 {
        db.query_sql_with_params(
            "INSERT INTO vectors (id, embedding) VALUES ($1, $2)",
            &[
                Value::Int(id),
                Value::Vector(Vector::new(&[1.0, 0.0]).unwrap()),
            ],
        )
        .unwrap();
    }
    let limited = VectorIndexBuildOptions {
        max_documents: NonZeroUsize::new(1).unwrap(),
        ..Default::default()
    };
    assert!(matches!(
        db.create_vector_index(
            "limited",
            &sql_field(),
            fixture.0.join("limited"),
            registration(),
            limited
        ),
        Err(VectorIndexBuildError::Source(
            ProjectionFeedError::Callback(HawDBError::ResourceBudgetExceeded(_))
        ))
    ));
    let mut limited = VectorIndexBuildOptions::default();
    limited.feed.work = NonZeroUsize::new(1).unwrap();
    assert!(matches!(
        db.create_vector_index(
            "feed",
            &sql_field(),
            fixture.0.join("feed"),
            registration(),
            limited
        ),
        Err(VectorIndexBuildError::Source(
            ProjectionFeedError::BudgetExceeded(_)
        ))
    ));
    db.query_sql_with_params(
        "INSERT INTO vectors (id, embedding) VALUES ($1, $2)",
        &[
            Value::Int(3),
            Value::Vector(Vector::new(&[0.0, 0.0]).unwrap()),
        ],
    )
    .unwrap();
    assert!(db
        .create_vector_index(
            "zero",
            &sql_field(),
            fixture.0.join("zero"),
            registration(),
            Default::default()
        )
        .is_err());
    db.query_sql("DELETE FROM vectors WHERE id = 3").unwrap();
    let consumer = db
        .create_vector_index(
            "valid",
            &sql_field(),
            fixture.0.join("valid"),
            registration(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(distances(&mut db, &consumer), [0.0, 0.0, 0.0]);
}

#[test]
fn vector_index_build_preserves_empty_and_null_only_dimensions() {
    for with_null in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
            .unwrap();
        if with_null {
            db.query_sql("INSERT INTO vectors (id, embedding) VALUES (1, NULL)")
                .unwrap();
        }
        let consumer = db
            .create_vector_index(
                "empty",
                &sql_field(),
                fixture.0.join("empty"),
                registration(),
                Default::default(),
            )
            .unwrap();
        assert!(distances(&mut db, &consumer).is_empty());
        drop(consumer);
        drop(db);
        let mut db = fixture.database();
        let consumer = db
            .open_search_projection_consumer(
                &SearchProjectionConsumerId::new("empty").unwrap(),
                fixture.0.join("empty"),
            )
            .unwrap();
        assert_eq!(
            consumer
                .search_index()
                .unwrap()
                .vector_index_descriptor()
                .unwrap()
                .dimension()
                .get(),
            2
        );
        assert!(distances(&mut db, &consumer).is_empty());
    }
}

#[test]
fn vector_index_read_domain_rejects_new_source_and_preserves_old_snapshot() {
    use crate::VectorIndexReadError;

    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    db.query_sql_with_params(
        "INSERT INTO vectors (id, embedding) VALUES (1, $1)",
        &[Value::Vector(Vector::new(&[1.0, 0.0]).unwrap())],
    )
    .unwrap();
    let consumer = db
        .create_vector_index(
            "index",
            &sql_field(),
            fixture.0.join("index"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let old = db
        .begin_read_transaction()
        .unwrap()
        .bind_vector_index(&consumer, &permit)
        .unwrap();
    db.query_sql("DELETE FROM vectors WHERE id = 1").unwrap();
    assert!(matches!(
        db.begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit),
        Err(VectorIndexReadError::SourceEpochMismatch { index, source })
            if index == old.source_epoch() && source > index
    ));
    let foreign_fixture = Fixture::new();
    let foreign = foreign_fixture.database();
    assert!(matches!(
        foreign
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit),
        Err(VectorIndexReadError::DatabaseMismatch)
    ));
    drop(consumer);
    drop(db);
    let result = old
        .query_candidates(
            &permit,
            Default::default(),
            &[1.0, 0.0],
            NonZeroUsize::MIN,
            NonZeroUsize::MIN,
        )
        .unwrap();
    assert_eq!(result.hits().len(), 1);
    assert_eq!(result.hits()[0].distance, 0.0);
    assert_eq!(result.report().source_epoch, old.source_epoch());
    assert_eq!(result.report().generation, old.generation());
    assert_eq!(
        old.source.query_sql("SELECT id FROM vectors").unwrap().rows[0]["id"],
        Value::Int(1)
    );
}

#[test]
fn vector_index_read_domain_rejects_a_forged_schema_definition() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    let consumer = db
        .create_search_projection_consumer(
            SearchProjectionConsumerId::new("forged").unwrap(),
            fixture.0.join("forged"),
            registration(),
            |source, index| {
                let definition = source.resolve_vector_index(
                    "forged",
                    &sql_field(),
                    VectorDistance::Cosine,
                    VectorIndexBuildOptions::default().backend,
                )?;
                let mut digest = *definition.schema_digest();
                digest[0] ^= 1;
                index.bind_vector_index(VectorIndexDescriptor::new(
                    definition.name().into(),
                    definition.target().clone(),
                    definition.dimension(),
                    definition.distance(),
                    definition.backend(),
                    digest,
                )?)
            },
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    assert!(matches!(
        db.begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit),
        Err(crate::VectorIndexReadError::SchemaMismatch)
    ));
}

#[test]
fn vector_index_rebuild_replaces_updates_deletes_and_nulls_without_mixing_old_pins() {
    for graph in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        let field = if graph {
            db.query("CREATE NODE TABLE Document").unwrap();
            db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
                .unwrap();
            VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            }
        } else {
            db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
                .unwrap();
            sql_field()
        };
        for (id, coordinates) in [(1, [1.0, 0.0]), (2, [0.0, 1.0]), (3, [-1.0, 0.0])] {
            let vector = Value::Vector(Vector::new(&coordinates).unwrap());
            if graph {
                db.query_with_params(
                    "CREATE (:Document {id: $id, embedding: $q})",
                    &BTreeMap::from([("id".into(), Value::Int(id)), ("q".into(), vector)]),
                )
                .unwrap();
            } else {
                db.query_sql_with_params(
                    "INSERT INTO vectors (id, embedding) VALUES ($1, $2)",
                    &[Value::Int(id), vector],
                )
                .unwrap();
            }
        }
        let mut consumer = db
            .create_vector_index(
                "index",
                &field,
                fixture.0.join("index"),
                registration(),
                Default::default(),
            )
            .unwrap();
        let permit = db
            .search_projection_consumer_read_permit(
                RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
            )
            .unwrap();
        let old = db
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit)
            .unwrap();
        let orthogonal = Value::Vector(Vector::new(&[0.0, 1.0]).unwrap());
        let opposite = Value::Vector(Vector::new(&[-1.0, 0.0]).unwrap());
        if graph {
            db.query_with_params(
                "MATCH (n:Document) WHERE n.id = 1 SET n.embedding = $q",
                &BTreeMap::from([("q".into(), orthogonal)]),
            )
            .unwrap();
            db.query("MATCH (n:Document) WHERE n.id = 2 DELETE n")
                .unwrap();
            db.query("MATCH (n:Document) WHERE n.id = 3 SET n.embedding = NULL")
                .unwrap();
            db.query_with_params(
                "CREATE (:Document {id: 4, embedding: $q})",
                &BTreeMap::from([("q".into(), opposite)]),
            )
            .unwrap();
        } else {
            db.query_sql_with_params(
                "UPDATE vectors SET embedding = $1 WHERE id = 1",
                &[orthogonal],
            )
            .unwrap();
            db.query_sql("DELETE FROM vectors WHERE id = 2").unwrap();
            db.query_sql("UPDATE vectors SET embedding = NULL WHERE id = 3")
                .unwrap();
            db.query_sql_with_params(
                "INSERT INTO vectors (id, embedding) VALUES (4, $1)",
                &[opposite],
            )
            .unwrap();
            // A compatible schema change still requires a new schema digest.
            db.query_sql("ALTER TABLE vectors ADD COLUMN category TEXT")
                .unwrap();
        }
        assert!(db
            .catch_up_search_projection_consumer(&mut consumer, 10, 10, 10, |_, _| panic!(
                "legacy mapping must not run for a vector field"
            ))
            .is_err());
        let cancelled = VectorIndexBuildOptions::default();
        cancelled.task.cancellation().cancel();
        assert!(db.rebuild_vector_index(&mut consumer, cancelled).is_err());
        db.rebuild_vector_index(&mut consumer, Default::default())
            .unwrap();
        let current = db
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit)
            .unwrap();
        assert!(current.source_epoch() > old.source_epoch());
        assert!(current.generation() > old.generation());
        if !graph {
            assert_ne!(
                current.definition().schema_digest(),
                old.definition().schema_digest()
            );
        }
        for (read, expected) in [(&old, vec![0.0, 1.0, 2.0]), (&current, vec![1.0, 2.0])] {
            let result = read
                .query_candidates(
                    &permit,
                    Default::default(),
                    &[1.0, 0.0],
                    NonZeroUsize::new(10).unwrap(),
                    NonZeroUsize::new(10).unwrap(),
                )
                .unwrap();
            assert_eq!(
                result
                    .hits()
                    .iter()
                    .map(|hit| hit.distance)
                    .collect::<Vec<_>>(),
                expected
            );
            assert_eq!(result.report().generation, read.generation());
            assert_eq!(result.report().source_epoch, read.source_epoch());
        }
        let generation = current.generation();
        drop(current);
        drop(old);
        drop(consumer);
        drop(permit);
        drop(db);
        let mut db = fixture.database();
        let consumer = db
            .open_search_projection_consumer(
                &SearchProjectionConsumerId::new("index").unwrap(),
                fixture.0.join("index"),
            )
            .unwrap();
        assert_eq!(distances(&mut db, &consumer), [1.0, 2.0]);
        let permit = db
            .search_projection_consumer_read_permit(
                RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
            )
            .unwrap();
        assert_eq!(
            consumer.pin(&permit).unwrap().publication_generation(),
            generation
        );
    }
}

#[test]
fn vector_index_cypher_pipeline_preserves_distance_window_and_canonical_traversal() {
    use crate::{QueryRequest, VectorIndexLanguageBudget};
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE NODE TABLE Document").unwrap();
    db.query("CREATE PROPERTY ON NODE TABLE Document(embedding) TYPE VECTOR(2)")
        .unwrap();
    for (id, coordinates) in [(1, [1.0, 0.0]), (2, [0.0, 1.0]), (3, [-1.0, 0.0])] {
        db.query_with_params(
            "CREATE (:Document {id: $id, embedding: $q})",
            &BTreeMap::from([
                ("id".into(), Value::Int(id)),
                (
                    "q".into(),
                    Value::Vector(Vector::new(&coordinates).unwrap()),
                ),
            ]),
        )
        .unwrap();
    }
    db.query(
        "MATCH (a:Document), (b:Document) WHERE a.id = 1 AND b.id = 3 CREATE (a)-[:RELATED]->(b)",
    )
    .unwrap();
    let consumer = db
        .create_vector_index(
            "docs",
            &VectorIndexField::NodeProperty {
                table: "Document".into(),
                property: "embedding".into(),
            },
            fixture.0.join("index"),
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
    let budget = VectorIndexLanguageBudget {
        index_working_bytes: NonZeroUsize::new(32 << 20).unwrap(),
        index_result_bytes: NonZeroUsize::new(1 << 20).unwrap(),
    };
    let mut parameters = BTreeMap::from([
        ("q".into(), Value::Vector(Vector::new(&[1.0, 0.0]).unwrap())),
        ("k".into(), Value::Int(3)),
    ]);
    let query = "CALL vector_index_search('docs', $q, candidates := 8, topK := $k) YIELD node AS m, distance RETURN m.id AS id, distance ORDER BY distance, id";
    let mut rows = Vec::new();
    let report = read
        .query_cypher_streaming(
            &permit,
            budget,
            QueryRequest::new(query).with_params(&parameters),
            |row| {
                rows.push(row);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["distance"].clone())
            .collect::<Vec<_>>(),
        [Value::Float(0.0), Value::Float(1.0), Value::Float(2.0)]
    );
    assert_eq!(report.index().unwrap().generation, read.generation());
    assert_eq!(report.index().unwrap().top_k, 3);
    drop(report);
    let filtered = "CALL vector_index_search('docs', $q, candidates := 8, topK := $k) YIELD node AS m, distance WITH m, distance WHERE m.id = 3 RETURN m.id AS id, distance";
    parameters.insert("k".into(), Value::Int(2));
    let mut filtered_rows = Vec::new();
    read.query_cypher_streaming(
        &permit,
        budget,
        QueryRequest::new(filtered).with_params(&parameters),
        |row| {
            filtered_rows.push(row);
            Ok(())
        },
    )
    .unwrap();
    assert!(
        filtered_rows.is_empty(),
        "filtering must not refill the declared ANN window"
    );
    parameters.insert("k".into(), Value::Int(3));
    read.query_cypher_streaming(
        &permit,
        budget,
        QueryRequest::new(filtered).with_params(&parameters),
        |row| {
            filtered_rows.push(row);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(filtered_rows.len(), 1);
    assert_eq!(filtered_rows[0]["distance"], Value::Float(2.0));
    let traverse = "CALL vector_index_search('docs', $q, candidates := 8, topK := 1) YIELD node AS m, distance MATCH (m)-[:RELATED]->(n:Document) RETURN n.id AS id, distance";
    let mut related = Vec::new();
    read.query_cypher_streaming(
        &permit,
        budget,
        QueryRequest::new(traverse).with_params(&parameters),
        |row| {
            related.push(row);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(related.len(), 1);
    assert_eq!(related[0]["id"], Value::Int(3));
    assert_eq!(related[0]["distance"], Value::Float(0.0));
}

#[test]
fn vector_index_sql_pipeline_preserves_composite_keys_distance_and_post_filtering() {
    use crate::{QueryStreamOptions, VectorIndexLanguageBudget, VectorIndexSqlRequest};
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (tenant TEXT, id BIGINT, embedding VECTOR(2), PRIMARY KEY (tenant, id))").unwrap();
    for (tenant, id, coordinates) in [
        ("a\0b", 1, [1.0, 0.0]),
        ("other", 1, [0.0, 1.0]),
        ("a\0b", 2, [-1.0, 0.0]),
    ] {
        db.query_sql_with_params(
            "INSERT INTO vectors (tenant, id, embedding) VALUES ($1, $2, $3)",
            &[
                Value::String(tenant.into()),
                Value::Int(id),
                Value::Vector(Vector::new(&coordinates).unwrap()),
            ],
        )
        .unwrap();
    }
    db.query_sql("INSERT INTO vectors (tenant, id, embedding) VALUES ('null', 3, NULL)")
        .unwrap();
    let consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("index"),
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
    let budget = VectorIndexLanguageBudget {
        index_working_bytes: NonZeroUsize::new(32 << 20).unwrap(),
        index_result_bytes: NonZeroUsize::new(1 << 20).unwrap(),
    };
    let mut parameters = vec![
        Value::Vector(Vector::new(&[1.0, 0.0]).unwrap()),
        Value::Int(3),
    ];
    let query = "SELECT v.tenant, v.id, distance FROM vector_index_search('rows', $1, candidates => 8, top_k => $2) AS v ORDER BY distance, v.id";
    let mut rows = Vec::new();
    let report = read
        .query_sql_streaming(
            &permit,
            budget,
            VectorIndexSqlRequest {
                sql: query,
                parameters: &parameters,
                output_limits: QueryStreamOptions::default(),
                task: Default::default(),
            },
            |row| {
                rows.push(row);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["distance"].clone())
            .collect::<Vec<_>>(),
        [Value::Float(0.0), Value::Float(1.0), Value::Float(2.0)]
    );
    assert_eq!(rows[0]["tenant"], Value::String("a\0b".into()));
    assert_eq!(rows[1]["tenant"], Value::String("other".into()));
    assert_eq!(report.index().generation, read.generation());
    assert_eq!(report.output_rows, 3);
    drop(report);
    let filtered = "SELECT v.id, distance FROM vector_index_search('rows', $1, candidates => 8, top_k => $2) AS v WHERE v.id = 2 ORDER BY distance LIMIT 1";
    for (k, expected) in [(2, 0), (3, 1)] {
        parameters[1] = Value::Int(k);
        let mut filtered_rows = Vec::new();
        read.query_sql_streaming(
            &permit,
            budget,
            VectorIndexSqlRequest {
                sql: filtered,
                parameters: &parameters,
                output_limits: QueryStreamOptions::default(),
                task: Default::default(),
            },
            |row| {
                filtered_rows.push(row);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(filtered_rows.len(), expected);
        if expected == 1 {
            assert_eq!(filtered_rows[0]["distance"], Value::Float(2.0));
        }
    }
    assert!(
        db.begin_read_transaction()
            .unwrap()
            .query_sql_with_params(query, &parameters)
            .is_err(),
        "ordinary SQL execution cannot silently consume an unbound vector index"
    );
}

#[test]
fn vector_index_sql_denies_wide_canonical_rows_before_delivery_and_can_retry() {
    use crate::{
        QueryStreamOptions, VectorIndexLanguageBudget, VectorIndexReadError, VectorIndexSqlRequest,
    };
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2), body TEXT)")
        .unwrap();
    db.query_sql_with_params(
        "INSERT INTO vectors (id, embedding, body) VALUES ($1, $2, $3)",
        &[
            Value::Int(1),
            Value::Vector(Vector::new(&[1.0, 0.0]).unwrap()),
            Value::String("x".repeat(256 * 1024)),
        ],
    )
    .unwrap();
    let consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("index"),
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
    let parameters = [Value::Vector(Vector::new(&[1.0, 0.0]).unwrap())];
    for (working, succeeds) in [(32 * 1024 * 1024 + 64 * 1024, false), (128 << 20, true)] {
        let task = RuntimeTaskContext::default()
            .with_memory_reservation(RuntimeMemoryReservation::new(working, 4 << 20));
        let mut delivered = 0;
        let result = read.query_sql_streaming(&permit, VectorIndexLanguageBudget {
            index_working_bytes: NonZeroUsize::new(32 << 20).unwrap(),
            index_result_bytes: NonZeroUsize::new(1 << 20).unwrap(),
        }, VectorIndexSqlRequest {
            sql: "SELECT body, distance FROM vector_index_search('rows', $1, candidates => 8, top_k => 1)",
            parameters: &parameters, output_limits: QueryStreamOptions::default(), task,
        }, |row| { assert_eq!(row["body"], Value::String("x".repeat(256 * 1024))); delivered += 1; Ok(()) });
        if succeeds {
            assert_eq!(result.unwrap().output_rows, 1);
            assert_eq!(delivered, 1);
        } else {
            assert!(
                matches!(
                    result,
                    Err(VectorIndexReadError::Query(
                        HawDBError::ResourceBudgetExceeded(_)
                    ))
                ),
                "{result:?}"
            );
            assert_eq!(delivered, 0);
        }
    }
}

#[test]
fn vector_index_rebuild_producer_budget_refusal_keeps_selected_generation() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    // Even an empty projection must fund its fixed compression workspace.
    let path = fixture.0.join("index");
    let mut consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            &path,
            registration(),
            Default::default(),
        )
        .unwrap();
    let result = db.rebuild_vector_index(
        &mut consumer,
        VectorIndexBuildOptions {
            memory_bytes: NonZeroUsize::new(4 << 20).unwrap(),
            ..Default::default()
        },
    );
    assert!(
        matches!(
            result,
            Err(VectorIndexBuildError::Consumer(
                crate::SearchProjectionConsumerError::Database(HawDBError::ResourceBudgetExceeded(
                    _
                ))
            ))
        ),
        "{result:?}"
    );
    assert!(distances(&mut db, &consumer).is_empty());
    let before = consumer.projection().receipt();
    db.rebuild_vector_index(&mut consumer, Default::default())
        .unwrap();
    let after = consumer.projection().receipt();
    assert_eq!(before.source_epoch, after.source_epoch);
    assert_ne!(
        before.binding.checkpoint_uuid,
        after.binding.checkpoint_uuid
    );
    drop(consumer);
    drop(db);
    let mut db = fixture.database();
    let consumer = db
        .open_search_projection_consumer(&SearchProjectionConsumerId::new("rows").unwrap(), path)
        .unwrap();
    assert!(distances(&mut db, &consumer).is_empty());
}

#[test]
fn vector_index_read_domain_rejects_another_branch_of_the_same_database() {
    use crate::{BranchCreateRequest, BranchSelector, VectorIndexReadError};
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    let consumer = db
        .create_vector_index(
            "rows",
            &sql_field(),
            fixture.0.join("index"),
            registration(),
            Default::default(),
        )
        .unwrap();
    let permit = db
        .search_projection_consumer_read_permit(
            RuntimeWorkRequest::foreground_query(64 << 20, 1 << 20).with_io_wave_slots(1),
        )
        .unwrap();
    let child = db
        .create_branch(BranchCreateRequest {
            name: Some("vector-child".into()),
            parent: BranchSelector::Name("main".into()),
            expected_source_commit_epoch: db.commit_epoch().unwrap(),
            owner: None,
            idempotency_key: "vector-child".into(),
        })
        .unwrap();
    db.use_branch(BranchSelector::Id(child.id)).unwrap();
    assert!(matches!(
        db.begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &permit),
        Err(VectorIndexReadError::BranchMismatch)
    ));
}

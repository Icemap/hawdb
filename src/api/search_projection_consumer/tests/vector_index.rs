// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Kernel acceptance; language-driven source mapping is tested separately.

use super::*;
use crate::{VectorDistance, VectorIndexBackend, VectorIndexDescriptor, VectorIndexField};
use hawdb_qos::RuntimeWorkRequest;
use hawdb_search::{SearchDocument, SearchQueryError, VectorIndexQuery};
use std::num::NonZeroUsize;

fn initialize_bound_index(
    db: &mut Database,
    path: &Path,
) -> (SearchProjectionConsumer, VectorIndexDescriptor) {
    db.query_sql("CREATE TABLE vectors (id TEXT PRIMARY KEY, embedding VECTOR(2))")
        .unwrap();
    let binding = db
        .begin_read_transaction()
        .unwrap()
        .resolve_vector_index(
            "vectors.embedding",
            &VectorIndexField::SqlColumn {
                table: "vectors".into(),
                column: "embedding".into(),
            },
            VectorDistance::Cosine,
            VectorIndexBackend::RaBitQ {
                bit_width: 4,
                segment_rows: 64,
                transform_seed: 42,
            },
        )
        .unwrap();
    let consumer = db
        .create_search_projection_consumer(
            id("vectors.embedding"),
            path,
            options(100),
            |_, index| {
                index.bind_vector_index(binding.clone())?;
                for (id, embedding) in [
                    ("a", Some(vec![1.0, 0.0])),
                    ("b", Some(vec![2.0, 0.0])),
                    ("c", Some(vec![0.0, 1.0])),
                    ("d", Some(vec![-1.0, 1.0])),
                    ("e", Some(vec![-1.0, 0.0])),
                    ("null", None),
                ] {
                    index.upsert(SearchDocument {
                        id: id.into(),
                        title: String::new(),
                        content: String::new(),
                        embedding,
                        metadata: Default::default(),
                    })?;
                }
                Ok(())
            },
        )
        .unwrap();
    (consumer, binding)
}

fn request(binding: &VectorIndexDescriptor) -> VectorIndexQuery<'_> {
    VectorIndexQuery {
        descriptor: binding,
        embedding: &[1.0, 0.0],
        candidate_limit: NonZeroUsize::new(8).unwrap(),
        top_k: NonZeroUsize::new(8).unwrap(),
    }
}

fn permit(db: &mut Database, working: u64, result: u64) -> hawdb_qos::RuntimePermit {
    db.search_projection_consumer_read_permit(
        RuntimeWorkRequest::foreground_query(working, result).with_io_wave_slots(1),
    )
    .unwrap()
}

#[test]
fn vector_index_kernel_preserves_signed_distances_and_retained_reopen_results() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let path = fixture.0.join("projection");
    let (consumer, binding) = initialize_bound_index(&mut db, &path);
    let epoch = consumer.projection().receipt().source_epoch;
    let admission = permit(&mut db, 64 << 20, 1 << 20);
    let pin = consumer.pin(&admission).unwrap();
    let output = pin
        .query_vector_index(&admission, Default::default(), request(&binding))
        .unwrap();
    let expected = [0.0, 0.0, 1.0, 1.0 + 1.0 / 2.0f64.sqrt(), 2.0];
    assert_eq!(
        output
            .hits()
            .iter()
            .map(|hit| hit.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b", "c", "d", "e"]
    );
    for (hit, distance) in output.hits().iter().zip(expected) {
        assert!((hit.distance - distance).abs() < 1e-12);
    }
    assert_eq!(output.report().source_epoch, epoch);
    assert_eq!(output.report().candidate_limit, 8);
    assert_eq!(output.report().reranked_candidate_count, 5);
    assert_eq!(output.report().backend, "rabitq");
    drop(pin);
    drop(admission);
    drop(consumer);
    drop(db);
    // Result ownership survives every query-side owner, including the database.
    assert_eq!(output.hits()[4].distance, 2.0);
    let mut db = fixture.database();
    let consumer = db
        .open_search_projection_consumer(&id("vectors.embedding"), &path)
        .unwrap();
    let admission = permit(&mut db, 64 << 20, 1 << 20);
    let pin = consumer.pin(&admission).unwrap();
    let reopened = pin
        .query_vector_index(&admission, Default::default(), request(&binding))
        .unwrap();
    assert_eq!(output.hits(), reopened.hits());
    assert_eq!(output.report().generation, reopened.report().generation);
    assert_eq!(
        consumer.search_index().unwrap().vector_index_descriptor(),
        Some(&binding)
    );
}

#[test]
fn vector_index_kernel_refuses_invalid_queries_and_recovers_after_admission_failure() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let (consumer, binding) = initialize_bound_index(&mut db, &fixture.0.join("projection"));
    for (working, result) in [(512, 1 << 20), (64 << 20, 1)] {
        let admission = permit(&mut db, working, result);
        let pin = consumer.pin(&admission).unwrap();
        assert!(matches!(
            pin.query_vector_index(&admission, Default::default(), request(&binding)),
            Err(SearchQueryError::Query(HawDBError::ResourceBudgetExceeded(
                _
            ))) | Err(SearchQueryError::Retention(_))
        ));
    }
    let admission = permit(&mut db, 64 << 20, 1 << 20);
    let pin = consumer.pin(&admission).unwrap();
    for embedding in [&[0.0, 0.0][..], &[1.0][..], &[f32::NAN, 0.0][..]] {
        assert!(pin
            .query_vector_index(
                &admission,
                Default::default(),
                VectorIndexQuery {
                    embedding,
                    ..request(&binding)
                }
            )
            .is_err());
    }
    assert!(pin
        .query_vector_index(
            &admission,
            Default::default(),
            VectorIndexQuery {
                candidate_limit: NonZeroUsize::new(1).unwrap(),
                ..request(&binding)
            }
        )
        .is_err());
    let cancelled = RuntimeTaskContext::default();
    cancelled.cancellation().cancel();
    assert!(pin
        .query_vector_index(&admission, cancelled, request(&binding))
        .is_err());
    let mut other = Database::new();
    let foreign = permit(&mut other, 64 << 20, 1 << 20);
    assert!(pin
        .query_vector_index(&foreign, Default::default(), request(&binding))
        .is_err());
    let result = pin
        .query_vector_index(&admission, Default::default(), request(&binding))
        .unwrap();
    assert_eq!(result.hits().len(), 5);
    assert_eq!(result.hits()[4].distance, 2.0);
}

#[test]
fn vector_index_rebuild_recovers_uncertain_selection_without_retargeting_old_reads() {
    for stage in [
        PublicationStage::AfterSelector,
        PublicationStage::AfterSelectorBarrier,
    ] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        db.query_sql("CREATE TABLE vectors (id BIGINT PRIMARY KEY, embedding VECTOR(2))")
            .unwrap();
        db.query_sql_with_params(
            "INSERT INTO vectors (id, embedding) VALUES ($1, $2)",
            &[
                hawdb_core::Value::Int(1),
                hawdb_core::Value::Vector(hawdb_core::Vector::new(&[1.0, 0.0]).unwrap()),
            ],
        )
        .unwrap();
        let path = fixture.0.join("projection");
        let mut consumer = db
            .create_vector_index(
                "uncertain-vector",
                &VectorIndexField::SqlColumn {
                    table: "vectors".into(),
                    column: "embedding".into(),
                },
                &path,
                options(100),
                Default::default(),
            )
            .unwrap();
        let admission = permit(&mut db, 64 << 20, 1 << 20);
        let old = db
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &admission)
            .unwrap();
        db.query_sql_with_params(
            "UPDATE vectors SET embedding = $1 WHERE id = 1",
            &[hawdb_core::Value::Vector(
                hawdb_core::Vector::new(&[-1.0, 0.0]).unwrap(),
            )],
        )
        .unwrap();
        PUBLICATION_FAILURE.set(Some((stage, false)));
        assert!(db
            .rebuild_vector_index(&mut consumer, Default::default())
            .is_err());
        let old_generation = old.generation();
        let old_output = old
            .query_candidates(
                &admission,
                Default::default(),
                &[1.0, 0.0],
                NonZeroUsize::new(8).unwrap(),
                NonZeroUsize::MIN,
            )
            .unwrap();
        assert_eq!(old_output.hits()[0].distance, 0.0);
        drop(old_output);
        drop(consumer);
        drop(db);
        let mut db = fixture.database();
        // Recovery cannot take project metadata authority from a retained
        // canonical transaction. Its refusal must not discard pending evidence.
        assert!(matches!(
            db.open_search_projection_consumer(&id("uncertain-vector"), &path),
            Err(Error::Database(HawDBError::BranchBusy { .. }))
        ));
        let old_output = old
            .query_candidates(
                &admission,
                Default::default(),
                &[1.0, 0.0],
                NonZeroUsize::new(8).unwrap(),
                NonZeroUsize::MIN,
            )
            .unwrap();
        assert_eq!(old_output.hits()[0].distance, 0.0);
        drop(old_output);
        drop(old);
        drop(admission);
        let consumer = db
            .open_search_projection_consumer(&id("uncertain-vector"), &path)
            .unwrap();
        let current_permit = permit(&mut db, 64 << 20, 1 << 20);
        let current = db
            .begin_read_transaction()
            .unwrap()
            .bind_vector_index(&consumer, &current_permit)
            .unwrap();
        assert!(current.generation() > old_generation);
        let result = current
            .query_candidates(
                &current_permit,
                Default::default(),
                &[1.0, 0.0],
                NonZeroUsize::new(8).unwrap(),
                NonZeroUsize::MIN,
            )
            .unwrap();
        assert_eq!(result.hits().len(), 1);
        assert_eq!(result.hits()[0].distance, 2.0);
    }
}

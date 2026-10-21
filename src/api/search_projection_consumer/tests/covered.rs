// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::*;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

fn nz(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}
fn document(value: &str) -> SearchDocument {
    SearchDocument {
        id: "memory:m1".into(),
        title: String::new(),
        content: value.into(),
        embedding: None,
        metadata: BTreeMap::new(),
    }
}
fn config() -> ProjectionConsumerCatchUpConfig {
    ProjectionConsumerCatchUpConfig {
        reads: PreparedReadLimits {
            dependency_entries: nz(64),
            dependency_bytes: nz(8192),
            statement_rows: nz(4),
            statement_payload_bytes: nz(4096),
        },
        feed: ProjectionFeedLimits {
            items: nz(1000),
            work: nz(10000),
            bytes: nz(65536),
        },
        mapping: ProjectionMappingLimits {
            calls: nz(1000),
            queries: nz(1000),
            reserved_rows: nz(4000),
            reserved_payload_bytes: nz(4 << 20),
            queries_per_call: nz(1),
            input_bytes: nz(4096),
            continuation_bytes: nz(128),
        },
        sorting: Default::default(),
        task: Default::default(),
    }
}
fn mapping(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    match input {
        ProjectionMappingInput::GraphNode { node_id, .. } => {
            let rows = ctx.query_with_params(
                "MATCH (m:Memory) WHERE id(m)=$id RETURN m.content AS content",
                &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
            )?;
            if let Some(row) = rows.output.rows.first() {
                let Value::String(content) = &row["content"] else {
                    panic!("text content")
                };
                ctx.upsert(document(content))?;
            }
        }
        ProjectionMappingInput::DeleteDocument { document_id, .. } => ctx.delete(document_id)?,
        _ => {}
    }
    Ok(ProjectionMappingStatus::Done)
}
fn full(
    ctx: &mut ProjectionBaselineMappingContext<'_, '_>,
    input: ProjectionFullSourceItem<'_>,
) -> Result<Option<SearchDocument>> {
    let ProjectionFullSourceItem::GraphNodeCandidate { node_id } = input else {
        return Ok(None);
    };
    let rows = ctx.query_with_params(
        "MATCH (m:Memory) WHERE id(m)=$id RETURN m.content AS content",
        &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
    )?;
    let Some(row) = rows.output.rows.first() else {
        return Ok(None);
    };
    let Value::String(content) = &row["content"] else {
        panic!("text content")
    };
    Ok(Some(document(content)))
}
fn prepare(
    db: &mut Database,
    consumer: &SearchProjectionConsumer,
) -> ProjectionCoveredConsumerUpdate {
    let cfg = config();
    let mut session = db
        .begin_search_projection_consumer_catch_up(consumer, cfg.clone(), mapping)
        .unwrap();
    for _ in 0..10000 {
        if session.advance(cfg.feed, Default::default()).unwrap()
            == ProjectionStagingSessionStep::Complete
        {
            break;
        }
    }
    let mut coverage = session
        .verify_complete_source(
            ProjectionCoverageConfig {
                feed: cfg.feed,
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
            },
            full,
        )
        .unwrap();
    for _ in 0..10000 {
        if coverage.advance(cfg.feed, Default::default()).unwrap()
            == ProjectionCoverageStep::Complete
        {
            break;
        }
    }
    coverage.finish().unwrap()
}

#[test]
fn registered_stationary_covered_catch_up_reuses_one_background_admission() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let governor = hawdb_qos::RuntimeGovernor::new(
        hawdb_qos::RuntimeGovernorConfig {
            background_task_limit: Some(nz(1)),
            memory_budget_bytes: Some(256 << 20),
            ..hawdb_qos::RuntimeGovernorConfig::shared_host()
        },
        hawdb_qos::RuntimeResourceSnapshot::from_parts(
            hawdb_qos::RuntimeResourceBudget::from_limits(nz(4), None, None),
            hawdb_qos::RuntimeMemorySnapshot::from_limits(
                Some(1 << 30),
                Some(1 << 30),
                None,
                None,
                None,
            ),
        ),
        hawdb_qos::IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    db.set_runtime_governor(governor.clone());
    append(&mut db, 1);
    let mut consumer = db
        .create_search_projection_consumer(
            id("covered-single-admission-v1"),
            fixture.0.join("projection"),
            options(1000),
            |_, index| index.upsert(document("document 1")),
        )
        .unwrap();
    db.query("MATCH (m:Memory) SET m.content='covered value'")
        .unwrap();
    let epoch = db.commit_epoch().unwrap();
    let covered = prepare(&mut db, &consumer);
    assert_eq!(governor.snapshot().active_background_tasks, 1);
    assert!(governor
        .try_admit(hawdb_qos::RuntimeWorkRequest::background_maintenance(1))
        .is_err());
    let report = db
        .publish_search_projection_consumer_covered_update(
            &mut consumer,
            covered,
            ProjectionSourceDurability::RequireKnownDurable,
        )
        .unwrap();
    assert!(report.catch_up.complete);
    assert_eq!(consumer.projection().receipt().source_epoch, epoch);
    assert_eq!(
        consumer
            .search_index()
            .unwrap()
            .document("memory:m1")
            .unwrap()
            .content,
        "covered value"
    );
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
}

#[test]
fn registered_stationary_covered_publication_faults_resolve_original_operation_after_source_writes()
{
    for stage in [
        PublicationStage::AfterCheckpoint,
        PublicationStage::BeforeSelector,
        PublicationStage::AfterSelector,
        PublicationStage::AfterSelectorBarrier,
        PublicationStage::BeforeRegistry,
        PublicationStage::AfterRegistry,
    ] {
        for reopen in [false, true] {
            let fixture = Fixture::new();
            let mut db = fixture.database();
            append(&mut db, 1);
            let path = fixture.0.join("projection");
            let mut consumer = db
                .create_search_projection_consumer(
                    id("covered-fault-v1"),
                    &path,
                    options(1000),
                    |_, index| index.upsert(document("document 1")),
                )
                .unwrap();
            let initial = consumer.projection().receipt();
            let permit = db
                .search_projection_consumer_read_permit(
                    hawdb_qos::RuntimeWorkRequest::foreground_query(16 << 20, 1 << 20)
                        .with_io_wave_slots(1),
                )
                .unwrap();
            let old = consumer.pin(&permit).unwrap();
            drop(permit);
            db.query("MATCH (m:Memory) SET m.content='covered value'")
                .unwrap();
            let e = db.commit_epoch().unwrap();
            let covered = prepare(&mut db, &consumer);
            PUBLICATION_FAILURE.set(Some((stage, false)));
            let failure = db
                .publish_search_projection_consumer_covered_update(
                    &mut consumer,
                    covered,
                    ProjectionSourceDurability::RequireKnownDurable,
                )
                .expect_err("injected publication failure");
            assert!(
                matches!(&failure, ProjectionConsumerCatchUpError::Consumer(
                    SearchProjectionConsumerError::Database(HawDBError::Storage(message))
                ) if message == &format!("injected consumer publication failure at {stage:?}")),
                "{stage:?}, reopen={reopen}: {failure:?}"
            );
            let selected = !matches!(
                stage,
                PublicationStage::AfterCheckpoint | PublicationStage::BeforeSelector
            );
            if !matches!(
                stage,
                PublicationStage::BeforeRegistry | PublicationStage::AfterRegistry
            ) {
                assert!(matches!(
                    consumer.search_index(),
                    Err(HawDBError::ProjectionBusy)
                ));
            }
            assert_eq!(old.report().source_epoch, initial.source_epoch);
            db.query("MATCH (m:Memory) SET m.content='later source'")
                .unwrap();
            if reopen {
                let governor = db.consumer_governor().unwrap();
                drop(consumer);
                drop(db);
                db = fixture.database();
                // Retained native pins belong to the original host governor.
                db.set_runtime_governor(governor);
                consumer = db
                    .open_or_create_search_projection_consumer(
                        id("covered-fault-v1"),
                        &path,
                        options(1000),
                        |_, _| panic!("recovery must retain the original registration"),
                    )
                    .unwrap();
            } else {
                db.resolve_search_projection_consumer_publication(&mut consumer)
                    .unwrap();
            }
            assert_eq!(
                consumer.projection().receipt().source_epoch,
                if selected { e } else { initial.source_epoch }
            );
            assert_eq!(
                consumer
                    .search_index()
                    .unwrap()
                    .document("memory:m1")
                    .unwrap()
                    .content,
                if selected {
                    "covered value"
                } else {
                    "document 1"
                }
            );
            assert_eq!(
                consumer.projection().receipt().binding.registration_uuid,
                initial.binding.registration_uuid
            );
            let covered = prepare(&mut db, &consumer);
            db.publish_search_projection_consumer_covered_update(
                &mut consumer,
                covered,
                ProjectionSourceDurability::Synchronize,
            )
            .unwrap();
            assert_eq!(
                consumer
                    .search_index()
                    .unwrap()
                    .document("memory:m1")
                    .unwrap()
                    .content,
                "later source"
            );
            drop(old);
        }
    }
}

#[test]
fn registered_stationary_covered_wrong_source_instance_cannot_publish() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let path = fixture.0.join("projection");
    let mut consumer = db
        .create_search_projection_consumer(
            id("covered-source-v1"),
            &path,
            options(1000),
            |_, index| index.upsert(document("document 1")),
        )
        .unwrap();
    let before = consumer.projection().receipt();
    let covered = prepare(&mut db, &consumer);
    // Reopening preserves durable registration identity but changes source-instance authority.
    drop(db);
    let mut db = fixture.database();
    assert!(matches!(
        db.publish_search_projection_consumer_covered_update(
            &mut consumer,
            covered,
            ProjectionSourceDurability::Synchronize,
        ),
        Err(ProjectionConsumerCatchUpError::Publication(
            ProjectionPublicationError::SourceChanged
        ))
    ));
    assert_eq!(consumer.projection().receipt(), before);
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

fn open_embedded(root: &Root) -> NowledgeMemEmbeddedStoreHandle {
    let mut config = DatabaseConfig {
        max_read_result_payload_bytes: Some(1 << 20),
        ..Default::default()
    };
    config.mutation_limits.max_result_payload_bytes = nz(1 << 20);
    config.execution_memory.query_memory_bytes = nz(64 << 20);
    config.execution_memory.blocking_operator_bytes = nz(16 << 20);
    let db = Database::open_with_config(root.0.join("graph"), config).unwrap();
    NowledgeMemEmbeddedStoreHandle::new(NowledgeMemEmbeddedStore::new(
        NowledgeMemGraph::from_database_with_runtime_governor(
            db,
            NowledgeMemGraphMode::WritableCutover,
            governor(),
        ),
        None,
    ))
    .unwrap()
}

fn rebuild(snapshot: &mut DatabaseReadTransaction, index: &mut SearchIndex) -> Result<()> {
    for row in snapshot
        .query("MATCH (n:Memory) RETURN n.id AS id,n.value AS value ORDER BY id LIMIT 8")?
        .rows
    {
        let Value::String(id) = &row["id"] else {
            panic!("text ID")
        };
        let Value::Int(value) = row["value"] else {
            panic!("integer value")
        };
        index.upsert(doc(&format!("memory:{id}"), &value.to_string()))?;
    }
    for row in snapshot
        .query_sql("SELECT id,body FROM records ORDER BY id LIMIT 8")?
        .rows
    {
        let (Value::String(id), Value::String(body)) = (&row["id"], &row["body"]) else {
            panic!("text row")
        };
        index.upsert(doc(&format!("record:{id}"), body))?;
    }
    Ok(())
}

fn acquire(
    handle: &NowledgeMemEmbeddedStoreHandle,
    root: &Root,
) -> NowledgeMemSearchProjectionConsumer {
    handle
        .open_or_create_search_projection_consumer(
            SearchProjectionConsumerId::new("embedded-graph-sql-v1").unwrap(),
            root.0.join("embedded-consumer"),
            SearchProjectionConsumerOptions::new(NonZeroU64::new(1000).unwrap()),
            rebuild,
        )
        .unwrap()
}

fn scope() -> ProjectionSourceScope {
    ProjectionSourceScope::Domains {
        node_labels: vec!["Memory".into()],
        relational_tables: vec!["records".into()],
    }
}

fn finish_private(
    consumer: &NowledgeMemSearchProjectionConsumer,
) -> ProjectionCoveredConsumerUpdate {
    let before = consumer.consumer().projection().receipt();
    let mut session = consumer
        .begin_catch_up(consumer_config(), delta_e, scope())
        .unwrap();
    let mut complete = false;
    for _ in 0..10000 {
        let step = session.advance(one(), sort()).unwrap();
        assert_eq!(consumer.consumer().projection().receipt(), before);
        if step == ProjectionStagingSessionStep::Complete {
            complete = true;
            break;
        }
    }
    assert!(complete);
    let covered = scoped_consumer_covered(session);
    assert_eq!(consumer.consumer().projection().receipt(), before);
    covered
}

#[test]
fn registered_consumer_embedded_one_operation_resume_rebuild_and_reopen() {
    let (root, mut db, original) = setup_consumer(false);
    drop(original);
    db.checkpoint().unwrap();
    drop(db);
    let handle = open_embedded(&root);
    let consumer = acquire(&handle, &root);
    let initial = consumer.consumer().projection().receipt();
    handle
        .with_transaction(|tx| {
            tx.query("MATCH (n:Memory) SET n.value=2")?;
            Ok(())
        })
        .unwrap();
    handle
        .with_transaction(|tx| {
            tx.query_sql("UPDATE records SET body='updated' WHERE id='a'")?;
            Ok(())
        })
        .unwrap();
    let mut partial = consumer
        .begin_catch_up(consumer_config(), delta_e, scope())
        .unwrap();
    let epoch = partial.source_epoch();
    assert_eq!(epoch, initial.source_epoch + 2);
    assert_ne!(
        partial.advance(one(), sort()).unwrap(),
        ProjectionStagingSessionStep::Complete
    );
    assert_eq!(consumer.consumer().projection().receipt(), initial);
    // Dropping partial work and every runtime owner is an explicit restart:
    // only the durable completed C survives, never the private prefix cursor.
    drop(partial);
    drop(consumer);
    drop(handle);
    let handle = open_embedded(&root);
    let mut consumer = acquire(&handle, &root);
    assert_eq!(consumer.consumer().projection().receipt(), initial);
    let covered = finish_private(&consumer);
    assert_eq!(covered.source_epoch(), epoch);
    let expected = handle
        .open_or_create_search_projection_consumer(
            SearchProjectionConsumerId::new("embedded-oracle-v1").unwrap(),
            root.0.join("embedded-oracle"),
            SearchProjectionConsumerOptions::new(NonZeroU64::new(1000).unwrap()),
            rebuild,
        )
        .unwrap();
    assert_eq!(
        expected.consumer().projection().receipt().source_epoch,
        epoch
    );
    let prepared = consumer
        .prepare_covered_update(covered, ProjectionSourceDurability::RequireKnownDurable)
        .unwrap();
    // A clone actually commits through the same embedded owner after E.
    let writer = handle.clone();
    std::thread::spawn(move || {
        writer.with_transaction(|tx| {
            tx.query_sql("INSERT INTO unrelated (id) VALUES ('post-e')")?;
            Ok(())
        })
    })
    .join()
    .unwrap()
    .unwrap();
    drop(handle);
    let report = prepared.publish().unwrap();
    assert_eq!(report.catch_up.end_durable_epoch, Some(epoch));
    assert_eq!(
        report.catch_up.start_durable_epoch,
        Some(initial.source_epoch)
    );
    let actual = consumer.consumer().search_index().unwrap();
    let expected_index = expected.consumer().search_index().unwrap();
    assert_eq!(actual.document_count(), expected_index.document_count());
    for id in ["memory:a", "record:a"] {
        assert_eq!(actual.document(id), expected_index.document(id));
    }
    let receipt = consumer.consumer().projection().receipt();
    assert_eq!(
        receipt.binding.registration_uuid,
        initial.binding.registration_uuid
    );
    drop(expected);
    drop(consumer);
    let handle = open_embedded(&root);
    let consumer = acquire(&handle, &root);
    assert_eq!(consumer.consumer().projection().receipt(), receipt);
    assert_eq!(
        consumer
            .consumer()
            .search_index()
            .unwrap()
            .document("record:a")
            .unwrap()
            .content,
        "updated"
    );
}

#[test]
fn registered_consumer_embedded_cancellation_prepared_drop_and_conflict_retry() {
    let (root, db, original) = setup_consumer(false);
    drop(original);
    drop(db);
    let handle = open_embedded(&root);
    let mut consumer = acquire(&handle, &root);
    let initial = consumer.consumer().projection().receipt();
    for reserve_vector in [false, true] {
        let mut labels = Vec::with_capacity(if reserve_vector { 1024 } else { 1 });
        let mut label = String::with_capacity(if reserve_vector { 6 } else { 16384 });
        label.push_str("Memory");
        labels.push(label);
        assert!(matches!(
            consumer.begin_catch_up(
                consumer_config(),
                delta_e,
                ProjectionSourceScope::Domains {
                    node_labels: labels,
                    relational_tables: vec![],
                },
            ),
            Err(ProjectionConsumerCatchUpError::Feed(
                ProjectionFeedError::BudgetExceeded(ProjectionFeedResource::Bytes)
            ))
        ));
        assert_eq!(consumer.consumer().projection().receipt(), initial);
    }
    handle
        .with_transaction(|tx| {
            tx.query("MATCH (n:Memory) SET n.value=2")?;
            Ok(())
        })
        .unwrap();
    let cancel = RuntimeCancellationToken::new();
    let mut config = consumer_config();
    config.task = RuntimeTaskContext::without_deadline(cancel.clone());
    let mut session = consumer.begin_catch_up(config, delta_e, scope()).unwrap();
    session.advance(one(), sort()).unwrap();
    cancel.cancel();
    assert!(session.advance(one(), sort()).is_err());
    drop(session);
    assert_eq!(consumer.consumer().projection().receipt(), initial);
    for conflict in [false, true] {
        let covered = finish_private(&consumer);
        let prepared = consumer
            .prepare_covered_update(covered, ProjectionSourceDurability::RequireKnownDurable)
            .unwrap();
        if conflict {
            handle
                .with_transaction(|tx| {
                    tx.query_sql("UPDATE records SET body='conflict' WHERE id='a'")?;
                    Ok(())
                })
                .unwrap();
            assert!(matches!(
                prepared.publish(),
                Err(ProjectionConsumerCatchUpError::Publication(
                    ProjectionPublicationError::Dependency(_)
                ))
            ));
        } else {
            drop(prepared);
        }
        consumer.resolve_publication().unwrap();
        assert_eq!(consumer.consumer().projection().receipt(), initial);
    }
    let covered = finish_private(&consumer);
    consumer
        .prepare_covered_update(covered, ProjectionSourceDurability::RequireKnownDurable)
        .unwrap()
        .publish()
        .unwrap();
    assert_eq!(
        consumer
            .consumer()
            .search_index()
            .unwrap()
            .document("record:a")
            .unwrap()
            .content,
        "conflict"
    );
    consumer.renew().unwrap();
    assert!(consumer.readiness(None).unwrap().is_ready());
    assert_eq!(
        consumer
            .cleanup(&Default::default())
            .unwrap()
            .failed_generations,
        0
    );
    handle
        .unregister_search_projection_consumer(consumer.consumer().id())
        .unwrap();
    handle
        .unregister_search_projection_consumer(consumer.consumer().id())
        .unwrap();
    assert!(consumer.readiness(None).is_err());
}

#[test]
fn registered_consumer_embedded_legacy_modes_keep_complete_interval_compatibility() {
    for incremental in [false, true] {
        let root = Root::new();
        let mut db = Database::open(root.0.join("graph")).unwrap();
        db.query("CREATE (:Memory {id:'a',title:'old-a',content:'old-a'})")
            .unwrap();
        db.query("CREATE (:Memory {id:'b',title:'old-b',content:'old-b'})")
            .unwrap();
        db.checkpoint().unwrap();
        let epoch = db.commit_epoch().unwrap();
        let path = root.0.join("legacy");
        let mut index = SearchIndex::open(&path).unwrap();
        for id in ["a", "b"] {
            index
                .upsert(doc(&format!("memory:{id}"), &format!("old-{id}")))
                .unwrap();
        }
        index
            .apply_projection_delta(SearchProjectionDelta {
                source_graph_commit_epoch: Some(epoch),
                ..Default::default()
            })
            .unwrap();
        index.checkpoint().unwrap();
        drop(index);
        drop(db);
        let mut options = NowledgeMemOpenOptions::with_search_projection(
            root.0.join("graph"),
            &path,
            NowledgeMemGraphMode::WritableCutover,
        );
        if incremental {
            options = options.with_incremental_search_maintenance(Default::default());
        }
        let (handle, _) = NowledgeMemEmbeddedStoreHandle::open_with_options(options).unwrap();
        for id in ["a", "b"] {
            handle
                .with_transaction(|tx| {
                    tx.query_with_params(
                        "MATCH (m:Memory {id:$id}) SET m.content='updated'",
                        &BTreeMap::from([("id".into(), Value::String(id.into()))]),
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        let before = handle.runtime_status().unwrap().projection_freshness;
        assert!(handle
            .catch_up_search_projection_with_batch_hydrator(1, 1, 1, |_, _| Ok(Default::default()))
            .is_err());
        assert_eq!(
            handle.runtime_status().unwrap().projection_freshness,
            before
        );
        let report = handle
            .catch_up_search_projection_with_batch_hydrator(8, 8, 1, |_, _| Ok(Default::default()))
            .unwrap();
        assert!(report.complete);
        assert_eq!(report.end_durable_epoch, Some(epoch + 2));
    }
}

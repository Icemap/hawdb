// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;

#[test]
fn owned_relational_wal_decode_matches_shared_codec_and_rejects_corruption() {
    use crate::background::CheckpointDecodeContext;
    for length in [0, 1, 31, 65_535, 65_536, 65_537] {
        let transaction = RelationalTransaction {
            writes: vec![RelationalWrite::Insert {
                table: "messages".into(),
                rows: vec![RelationalRow::new(vec![
                    RelationalValue::Text("id-\0-中文".into()),
                    RelationalValue::Text("x".repeat(length)),
                    RelationalValue::Bytea(vec![0xff; length]),
                ])],
                mode: RelationalInsertMode::Error,
            }],
        };
        let bytes = encode_relational_wal_batch(17, &transaction).unwrap();
        let allocations = CheckpointDecodeContext {
            work: Default::default(),
            memory: Default::default(),
        };
        let ordinary = decode_relational_wal_batch(&bytes, RelationalDecodeLimits::wal()).unwrap();
        let owned =
            decode_relational_wal_batch_owned(&bytes, RelationalDecodeLimits::wal(), &allocations)
                .unwrap();
        assert_eq!(owned, ordinary);
        drop(owned);
        for offset in [0, 8, 28, bytes.len() - 1] {
            let mut corrupt = bytes.clone();
            corrupt[offset] ^= 0xff;
            assert!(decode_relational_wal_batch(&corrupt, RelationalDecodeLimits::wal()).is_err());
            assert!(decode_relational_wal_batch_owned(
                &corrupt,
                RelationalDecodeLimits::wal(),
                &allocations,
            )
            .is_err());
        }
    }
}

#[test]
fn owned_relational_wal_decode_denies_large_field_before_copy_and_releases_memory() {
    use crate::background::{CheckpointDecodeContext, CheckpointWorkContext};
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeTaskContext,
        RuntimeWorkRequest,
    };
    let transaction = RelationalTransaction {
        writes: vec![RelationalWrite::Insert {
            table: "messages".into(),
            rows: vec![RelationalRow::new(vec![RelationalValue::Text(
                "x".repeat(128 * 1024),
            )])],
            mode: RelationalInsertMode::Error,
        }],
    };
    let bytes = encode_relational_wal_batch(1, &transaction).unwrap();
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(4 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(4 * 1024))
        .unwrap();
    let allocations = CheckpointDecodeContext {
        work: CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default())),
        memory: Default::default(),
    };
    let observer = crate::test_allocator::AllocationObservation::start();
    let result =
        decode_relational_wal_batch_owned(&bytes, RelationalDecodeLimits::wal(), &allocations);
    let large_allocations = observer.finish();
    let error = result.unwrap_err();
    assert!(matches!(error, RelationalError::Admission(_)), "{error}");
    assert_eq!(large_allocations, 0, "deny before copying the large field");
    drop(allocations);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn owned_relational_wal_decode_preserves_all_write_variants_and_access_captures() {
    use crate::background::CheckpointDecodeContext;
    let key = RelationalKey(vec![RelationalValue::Text("id-1".into())]);
    let row = RelationalRow::new(vec![RelationalValue::Text("id-1".into())]);
    let predicate = RelationalPredicate::Not(Box::new(RelationalPredicate::IsNull {
        column: "id".into(),
        negated: false,
    }));
    let mut transaction = create_payload_table();
    transaction.writes.extend([
        RelationalWrite::CreateIndex {
            table: "messages".into(),
            index: RelationalIndexSchema {
                name: "by_id".into(),
                columns: vec!["id".into()],
                unique: false,
            },
        },
        RelationalWrite::Insert {
            table: "messages".into(),
            rows: vec![row.clone()],
            mode: RelationalInsertMode::Error,
        },
        RelationalWrite::DeleteByPrimaryKey {
            table: "messages".into(),
            keys: vec![key.clone()],
        },
        RelationalWrite::Upsert {
            table: "messages".into(),
            rows: vec![row],
            conflict_columns: vec!["id".into()],
            action: RelationalConflictAction::Update(vec![RelationalUpsertAssignment {
                column: "id".into(),
                value: RelationalUpsertValue::ExcludedColumn("id".into()),
            }]),
        },
        RelationalWrite::DeleteWhere {
            table: "messages".into(),
            predicate: predicate.clone(),
        },
        RelationalWrite::UpdateWhere {
            table: "messages".into(),
            assignments: vec![RelationalUpdateAssignment {
                column: "id".into(),
                value: RelationalUpdateValue::Value(RelationalValue::Text("id-2".into())),
            }],
            predicate,
        },
        RelationalWrite::AddColumn {
            table: "messages".into(),
            column: RelationalColumnSchema {
                name: "extra".into(),
                scalar_type: RelationalScalarType::Text,
                nullable: true,
                default: None,
            },
        },
    ]);
    let access = RelationalReplayAccessSet::from_decoded_entries(vec![RelationalReplayAccess {
        table: "messages".into(),
        primary_key: key.clone(),
    }])
    .unwrap();
    let capture = RelationalPrimaryKeyChangeCapture::Captured {
        tables: vec![RelationalTablePrimaryKeyChanges {
            table: "messages".into(),
            primary_keys: vec![key],
        }],
        encoded_bytes: 0,
    };
    let encoded =
        encode_relational_wal_batch_with_captures(9, &transaction, Some(&access), &capture)
            .unwrap();
    let allocations = CheckpointDecodeContext {
        work: Default::default(),
        memory: Default::default(),
    };
    let ordinary =
        decode_relational_wal_batch(&encoded.record, RelationalDecodeLimits::wal()).unwrap();
    let owned = decode_relational_wal_batch_owned(
        &encoded.record,
        RelationalDecodeLimits::wal(),
        &allocations,
    )
    .unwrap();
    assert_eq!(owned, ordinary);
    assert_eq!(owned.transaction, transaction);
    assert_eq!(owned.replay_access.as_ref(), Some(&access));
}

#[test]
fn planning_preserves_resident_detached_and_file_backed_overflow_terms() {
    let source = RelationalStore::with_overflow_config(
        RelationalMutationLimits::default(),
        RelationalOverflowConfig {
            threshold_bytes: 1,
            ..RelationalOverflowConfig::default()
        },
    );
    source
        .commit(create_payload_table(), |_, _| Ok(()))
        .unwrap();
    source
        .commit(
            RelationalTransaction {
                writes: vec![RelationalWrite::Insert {
                    table: "messages".into(),
                    rows: vec![RelationalRow::new(vec![
                        RelationalValue::Text("message-1".into()),
                        RelationalValue::Text("payload".repeat(64)),
                    ])],
                    mode: RelationalInsertMode::Error,
                }],
            },
            |_, _| Ok(()),
        )
        .unwrap();
    let checkpoint = source.encode_checkpoint().unwrap();
    let path = std::env::temp_dir().join(format!(
        "hawdb-admission-size-{}.hawdb",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    std::fs::write(&path, &checkpoint).unwrap();
    let snapshot = source.snapshot().unwrap();
    let decoded =
        decode_relational_checkpoint_file(&path, RelationalDecodeLimits::checkpoint()).unwrap();
    assert_eq!(decoded.state.file_backed_overflow_segment_count(), 1);
    let resident = (std::mem::size_of::<RelationalKey>()
        + std::mem::size_of::<RelationalRow>()
        + std::mem::size_of::<Vec<RelationalValue>>()
        + 2 * "message-1".len()
        + std::mem::size_of::<RelationalOverflowRef>()) as u64;
    for original in [snapshot.value(), &decoded.state] {
        let overflow_bytes = original
            .overflow_segments
            .values()
            .next()
            .unwrap()
            .read()
            .unwrap()
            .as_ref()
            .len() as u64;
        assert_eq!(original.estimated_materialized_row_bytes(), resident);
        assert_eq!(
            original.estimated_checkpoint_bytes(),
            resident + overflow_bytes
        );
        for detached in [false, true] {
            let mut state = original.clone();
            if detached {
                state.omit_materialized_rows();
            }
            let expected = (
                if detached { 0 } else { resident },
                resident + overflow_bytes,
            );
            let mut visits = 0;
            assert_eq!(
                state
                    .estimated_checkpoint_admission_bytes_with_visit(&mut || {
                        visits += 1;
                        Ok::<_, usize>(())
                    })
                    .unwrap(),
                expected
            );
            assert!(visits > 0);
            for denied in 1..=visits {
                let mut reached = 0;
                assert_eq!(
                    state.estimated_checkpoint_admission_bytes_with_visit(&mut || {
                        reached += 1;
                        if reached == denied {
                            Err(denied)
                        } else {
                            Ok(())
                        }
                    }),
                    Err(denied)
                );
                assert_eq!(reached, denied);
            }
            assert_eq!(state.estimated_materialized_row_bytes(), expected.0);
            assert_eq!(state.estimated_checkpoint_bytes(), expected.1);
        }
    }
    drop(decoded);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn controlled_metadata_clone_denies_wide_names_and_keeps_shared_count_ownership() {
    use crate::background::CheckpointWorkContext;
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeTaskContext,
        RuntimeWorkRequest,
    };
    let name = "t".repeat(128 * 1024);
    let mut transaction = create_upsert_table();
    let RelationalWrite::CreateTable(schema) = &mut transaction.writes[0] else {
        panic!("fixture schema");
    };
    schema.name = name.clone();
    let mut source = RelationalState::default()
        .stage_transaction(
            transaction,
            RelationalMutationLimits::default(),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    source.omit_materialized_rows();
    assert!(!source.materialized_rows_resident());
    for (ceiling, succeeds) in [(4096, false), (16 * 1024 * 1024, true)] {
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(ceiling),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        governor.pin_resources();
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(ceiling))
            .unwrap();
        let work =
            CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()));
        let observer = crate::test_allocator::AllocationObservation::start();
        let cloned = source.clone_metadata_with_work_context(&work);
        let wide_allocations = observer.finish();
        if succeeds {
            let mut cloned = cloned.unwrap();
            for _ in 0..32 {
                cloned = cloned.clone_metadata_with_work_context(&work).unwrap();
                assert!(
                    matches!(work.task_context().reserve_working_memory(ceiling),
                    Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                    if available_bytes > ceiling - 448 * 1024)
                );
            }
            assert_eq!(cloned.table_schema(&name), source.table_schema(&name));
            assert_eq!(cloned.row_count(&name), 0);
            assert!(!cloned
                .detached_row_counts
                .shares_storage_with(&source.detached_row_counts));
            assert!(
                matches!(work.task_context().reserve_working_memory(ceiling),
                Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                if available_bytes <= ceiling - 3 * 128 * 1024)
            );
            let retained = cloned.detached_row_counts.clone();
            drop(cloned);
            assert!(
                matches!(work.task_context().reserve_working_memory(ceiling),
                Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                if available_bytes <= ceiling - 128 * 1024 && available_bytes > ceiling - 160 * 1024)
            );
            assert_eq!(retained.get(&name), Some(&0));
            drop(retained);
        } else {
            assert!(matches!(cloned, Err(RelationalError::Admission(_))));
            assert_eq!(wide_allocations, 0);
        }
        assert!(
            matches!(work.task_context().reserve_working_memory(ceiling),
            Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. }) if available_bytes == ceiling)
        );
        assert!(source.table_schema(&name).is_some());
        assert_eq!(source.row_count(&name), 0);
        drop(work);
        drop(permit);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn controlled_metadata_clone_cancellation_keeps_source_and_refunds_inventory() {
    use crate::background::{CheckpointWorkContext, CheckpointWorkProbe};
    use hawdb_qos::{
        IoConcurrencyBudget, LocalQosPolicy, LocalQosScheduler, RuntimeGovernor,
        RuntimeGovernorConfig, RuntimeTaskContext, RuntimeWorkRequest,
    };
    let mut source = RelationalState::default()
        .stage_transaction(
            create_upsert_table(),
            RelationalMutationLimits::default(),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    source.omit_materialized_rows();
    let ceiling = 1024 * 1024;
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(ceiling),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling))
        .unwrap();
    let scheduler = LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..Default::default()
    });
    let probe = Arc::new(CheckpointWorkProbe::default());
    probe
        .cancel_after
        .store(2, std::sync::atomic::Ordering::SeqCst);
    scheduler.set_telemetry_sink(Some(probe.clone()));
    let task = permit.bind_task_context(RuntimeTaskContext::without_deadline(
        probe.cancellation.clone(),
    ));
    let work = CheckpointWorkContext::new(task).with_scheduler(scheduler.clone());
    let result = source.clone_metadata_with_work_context(&work);
    assert!(matches!(result, Err(RelationalError::Admission(_))));
    probe.assert_released(&scheduler);
    assert!(source.table_schema("documents").is_some());
    drop(work);
    let retry = CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()));
    let copied = source.clone_metadata_with_work_context(&retry).unwrap();
    assert_eq!(
        copied.table_schema("documents"),
        source.table_schema("documents")
    );
    drop(copied);
    drop(retry);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn controlled_sparse_workspace_denies_wide_key_before_copy_and_retains_only_workspace_nodes() {
    use crate::background::{CheckpointDecodeContext, CheckpointWorkContext};
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeTaskContext,
        RuntimeWorkRequest,
    };
    let source = RelationalState::default()
        .stage_transaction(
            create_upsert_table(),
            RelationalMutationLimits::default(),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    let id = "k".repeat(128 * 1024);
    let hydrated = RelationalSparseRecoveryRow {
        table: "documents".into(),
        primary_key: RelationalKey(vec![RelationalValue::Text(id.clone())]),
        row: Some(upsert_row(&id, "owner-1", "body")),
    };
    for (ceiling, succeeds) in [(4096, false), (16 * 1024 * 1024, true)] {
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(ceiling),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        governor.pin_resources();
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(ceiling))
            .unwrap();
        let work =
            CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()));
        let context = CheckpointDecodeContext {
            work: work.clone(),
            memory: Default::default(),
        };
        let input = vec![hydrated.clone()];
        let observer = crate::test_allocator::AllocationObservation::start();
        let result = source.sparse_workspace_with_allocations(
            input,
            RelationalRowChangeCaptureLimits::default(),
            "sparse relational recovery",
            Some(&context),
        );
        let wide_allocations = observer.finish();
        if succeeds {
            let (workspace, supplied) = result.unwrap();
            assert_eq!(workspace.row_count("documents"), 1);
            assert_eq!(supplied[0].primary_key, hydrated.primary_key);
            assert!(
                matches!(work.task_context().reserve_working_memory(ceiling),
                Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                if available_bytes <= ceiling - 128 * 1024)
            );
            drop(supplied);
            drop(context);
            assert!(
                matches!(work.task_context().reserve_working_memory(ceiling),
                Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                if available_bytes > ceiling - 32 * 1024 && available_bytes < ceiling)
            );
            drop(workspace);
        } else {
            assert!(matches!(result, Err(RelationalError::Admission(_))));
            assert_eq!(wide_allocations, 0);
            drop(context);
        }
        assert!(
            matches!(work.task_context().reserve_working_memory(ceiling),
            Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. }) if available_bytes == ceiling)
        );
        assert_eq!(source.row_count("documents"), 0);
        drop(work);
        drop(permit);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn controlled_sparse_workspace_moves_sorted_rows_into_bounded_pages() {
    use crate::background::CheckpointDecodeContext;
    let source = RelationalState::default()
        .stage_transaction(
            create_upsert_table(),
            RelationalMutationLimits::default(),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    let rows = (0..513)
        .map(|id| {
            let id = format!("id-{id:04}");
            RelationalSparseRecoveryRow {
                table: "documents".into(),
                primary_key: RelationalKey(vec![RelationalValue::Text(id.clone())]),
                row: Some(upsert_row(&id, "owner", "body")),
            }
        })
        .collect::<Vec<_>>();
    let (ordinary, expected_access) = source
        .sparse_workspace(
            rows.clone(),
            RelationalRowChangeCaptureLimits::default(),
            "sparse relational recovery",
        )
        .unwrap();
    let context = CheckpointDecodeContext {
        work: Default::default(),
        memory: Default::default(),
    };
    let (controlled, access) = source
        .sparse_workspace_with_allocations(
            rows,
            RelationalRowChangeCaptureLimits::default(),
            "sparse relational recovery",
            Some(&context),
        )
        .unwrap();
    assert_eq!(access, expected_access);
    assert_eq!(controlled.row_count("documents"), 513);
    let pages = &controlled.segments["documents"].rows.pages;
    assert_eq!(pages.len(), 3);
    assert!(pages
        .iter()
        .all(|page| page.len() <= RELATIONAL_ROW_PAGE_MAX_ENTRIES));
    for entry in access {
        assert_eq!(
            controlled.row("documents", &entry.primary_key),
            ordinary.row("documents", &entry.primary_key)
        );
    }
}

#[test]
fn controlled_column_positions_share_results_errors_and_deny_capacity_before_allocation() {
    use crate::background::{CheckpointDecodeContext, CheckpointWorkContext};
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeTaskContext,
        RuntimeWorkRequest,
    };
    let source = RelationalState::default()
        .stage_transaction(
            create_upsert_table(),
            RelationalMutationLimits::default(),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    let schema = source.table_schema("documents").unwrap();
    let context = CheckpointDecodeContext {
        work: Default::default(),
        memory: Default::default(),
    };
    for columns in [
        vec![],
        vec!["payload".into(), "id".into(), "id".into()],
        vec!["id".into(), "missing".into()],
    ] {
        let ordinary = column_positions(schema, &columns);
        let controlled = column_positions_with_allocations(schema, &columns, Some(&context));
        match (ordinary, controlled) {
            (Ok(expected), Ok(actual)) => assert_eq!(actual, expected),
            (Err(expected), Err(actual)) => assert_eq!(actual.to_string(), expected.to_string()),
            result => panic!("column-position drift: {result:?}"),
        }
    }
    let columns = vec!["id".to_string(); 16 * 1024];
    let ceiling = 4096;
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(ceiling),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    governor.pin_resources();
    let permit = governor
        .try_admit(RuntimeWorkRequest::background_maintenance(ceiling))
        .unwrap();
    let context = CheckpointDecodeContext {
        work: CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default())),
        memory: Default::default(),
    };
    let observer = crate::test_allocator::AllocationObservation::start();
    let denied = column_positions_with_allocations(schema, &columns, Some(&context));
    assert_eq!(observer.finish(), 0);
    assert!(matches!(denied, Err(RelationalError::Admission(_))));
    drop(context);
    drop(permit);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn controlled_row_page_copy_denies_before_wide_keys_and_retains_snapshot_leases() {
    use crate::background::CheckpointWorkContext;
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeTaskContext,
        RuntimeWorkRequest,
    };
    let id = "k".repeat(128 * 1024);
    let key = RelationalKey(vec![RelationalValue::Text(id.clone())]);
    let source = RelationalRowPages::from_map(BTreeMap::from([(
        key.clone(),
        upsert_row(&id, "owner", "body"),
    )]));
    for (ceiling, succeeds) in [(4096, false), (16 * 1024 * 1024, true)] {
        let mut target = source.clone();
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(ceiling),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        governor.pin_resources();
        let permit = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(ceiling))
            .unwrap();
        let work =
            CheckpointWorkContext::new(permit.bind_task_context(RuntimeTaskContext::default()));
        let observer = crate::test_allocator::AllocationObservation::start();
        let result = target.prepare_checkpoint_copy_for_key(&key, &work);
        let wide_allocations = observer.finish();
        if succeeds {
            result.unwrap();
            assert!(!target.pages.shares_storage_with(&source.pages));
            assert_eq!(target.shared_page_count(&source), 0);
            assert_eq!(target.get(&key), source.get(&key));
            let retained = target.clone();
            drop(target);
            assert!(
                matches!(work.task_context().reserve_working_memory(ceiling),
                Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. })
                if available_bytes <= ceiling - 128 * 1024)
            );
            assert_eq!(retained.get(&key), source.get(&key));
            drop(retained);
        } else {
            assert!(matches!(result, Err(RelationalError::Admission(_))));
            assert_eq!(wide_allocations, 0);
            assert!(target.pages.shares_storage_with(&source.pages));
            assert_eq!(target.shared_page_count(&source), 1);
            assert_eq!(target.get(&key), source.get(&key));
            drop(target);
        }
        assert!(
            matches!(work.task_context().reserve_working_memory(ceiling),
            Err(hawdb_core::RuntimeMemoryError::ReservationExceeded { available_bytes, .. }) if available_bytes == ceiling)
        );
        drop(work);
        drop(permit);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
}

#[test]
fn controlled_row_page_copy_cancellation_keeps_directory_and_retries() {
    use crate::background::CheckpointWorkProbe;
    use hawdb_qos::{LocalQosPolicy, LocalQosScheduler};
    let key = RelationalKey(vec![RelationalValue::Text("id".into())]);
    let source = RelationalRowPages::from_map(BTreeMap::from([(
        key.clone(),
        upsert_row("id", "owner", "body"),
    )]));
    let mut target = source.clone();
    let probe = Arc::new(CheckpointWorkProbe::default());
    probe
        .cancel_after
        .store(2, std::sync::atomic::Ordering::SeqCst);
    let scheduler = LocalQosScheduler::new(LocalQosPolicy {
        max_background_operations: Some(1),
        max_total_background_operations: Some(1),
        ..Default::default()
    });
    let work = probe.context(scheduler.clone());
    assert!(matches!(
        target.prepare_checkpoint_copy_for_key(&key, &work),
        Err(RelationalError::Admission(_))
    ));
    probe.assert_released(&scheduler);
    assert!(target.pages.shares_storage_with(&source.pages));
    assert_eq!(target.shared_page_count(&source), 1);
    assert_eq!(target.get(&key), source.get(&key));
    target
        .prepare_checkpoint_copy_for_key(&key, &Default::default())
        .unwrap();
    assert!(!target.pages.shares_storage_with(&source.pages));
    assert_eq!(target.get(&key), source.get(&key));
}

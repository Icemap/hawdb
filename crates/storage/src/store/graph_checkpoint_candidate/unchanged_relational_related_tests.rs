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
use crate::config::{DurabilityPolicy, RelationalIndexMode, WalReplayConfig};
use crate::relational::{
    RelationalColumnSchema, RelationalHydrationBudget, RelationalInsertMode, RelationalKey,
    RelationalRow, RelationalRowPageSnapshotReadLimits, RelationalRowPageSnapshotReader,
    RelationalScalarType, RelationalTableSchema, RelationalTransaction, RelationalValue,
    RelationalWrite,
};

fn insert(keys: &[&str]) -> RelationalWrite {
    RelationalWrite::Insert {
        table: "documents".into(),
        rows: keys
            .iter()
            .map(|key| RelationalRow::new(vec![RelationalValue::Text((*key).into())]))
            .collect(),
        mode: RelationalInsertMode::Error,
    }
}

fn assert_row(reader: &RelationalRowPageSnapshotReader, key: &str, present: bool) {
    let (row, report) = reader
        .point_projected(
            "documents",
            &RelationalKey(vec![RelationalValue::Text(key.into())]),
            &[0],
            RelationalRowPageSnapshotReadLimits::default(),
            &mut RelationalHydrationBudget::default(),
            &RuntimeTaskContext::default(),
        )
        .unwrap();
    assert_eq!(
        report.identity.visible_commit_epoch,
        reader.identity().visible_commit_epoch
    );
    assert_eq!(row.is_some(), present);
    if let Some(row) = row {
        assert_eq!(row.fields.len(), 1);
        assert_eq!(row.fields[0].value, RelationalValue::Text(key.into()));
    }
}

#[test]
fn relational_suffix_after_unchanged_prefix_seals_and_keeps_all_later_prefixes_bound() {
    for mode in [
        RelationalIndexMode::Shadow,
        RelationalIndexMode::Authoritative,
    ] {
        for policy in [
            DurabilityPolicy::SyncOnEveryWrite,
            DurabilityPolicy::SyncOnCheckpoint,
        ] {
            let root = std::env::temp_dir().join(format!(
                "hawdb-relational-prefix-transition-{}",
                hawdb_core::generate_uuidv7().unwrap()
            ));
            let mut catalog = Catalog::default();
            let replay = WalReplayConfig {
                relational_index_mode: mode,
                ..WalReplayConfig::default()
            };
            let mut store = GraphStore::open_with_durability_and_replay_config(
                &root,
                &mut catalog,
                policy,
                WalReplayConfig {
                    relational_index_mode: RelationalIndexMode::Shadow,
                    ..replay
                },
            )
            .unwrap();
            store
                .commit_relational_transaction(
                    &mut catalog,
                    RelationalTransaction {
                        writes: vec![
                            RelationalWrite::CreateTable(RelationalTableSchema {
                                name: "documents".into(),
                                columns: vec![RelationalColumnSchema {
                                    name: "id".into(),
                                    scalar_type: RelationalScalarType::Text,
                                    nullable: false,
                                    default: None,
                                }],
                                primary_key: vec!["id".into()],
                                unique_constraints: Vec::new(),
                                foreign_keys: Vec::new(),
                                indexes: Vec::new(),
                            }),
                            insert(&["kept"]),
                        ],
                    },
                )
                .unwrap();
            store.checkpoint(&catalog).unwrap();
            drop(store);
            let mut store = GraphStore::open_with_durability_and_replay_config(
                &root,
                &mut catalog,
                policy,
                replay,
            )
            .unwrap();
            store
                .create_node(&mut catalog, "Memory", BTreeMap::new())
                .unwrap();
            let pinned = store.checkpoint_source();
            let mut candidate = pinned
                .prepare_checkpoint_candidate(&catalog)
                .unwrap()
                .unwrap();
            store
                .create_node(&mut catalog, "Memory", BTreeMap::new())
                .unwrap();
            candidate.catch_up(&store).unwrap();
            candidate.finish_catch_up().unwrap();
            assert!(candidate.recovery_selectors.iter().all(Option::is_none));
            let old_reader = candidate
                .store
                .as_ref()
                .unwrap()
                .open_relational_row_snapshot_reader()
                .unwrap()
                .unwrap();
            let old_epoch = old_reader.identity().visible_commit_epoch;
            assert_row(&old_reader, "kept", true);
            assert_row(&old_reader, "new-a", false);
            // Both inserts are one complete SQL/WAL transaction, rather than
            // independent host writes or an artificial flag assignment.
            store
                .commit_relational_transaction(
                    &mut catalog,
                    RelationalTransaction {
                        writes: vec![insert(&["new-a"]), insert(&["new-b"])],
                    },
                )
                .unwrap();
            candidate.catch_up(&store).unwrap();
            candidate.finish_catch_up().unwrap();
            assert!(
                candidate.recovery_selectors.iter().all(Option::is_some),
                "the first relational suffix must prepare both recovery selectors"
            );
            // A later graph-only suffix must retain relational recovery and
            // bind its new complete source fence, including the earlier SQL.
            store
                .create_node(&mut catalog, "Memory", BTreeMap::new())
                .unwrap();
            candidate.catch_up(&store).unwrap();
            candidate.finish_catch_up().unwrap();
            assert!(
                candidate.recovery_selectors.iter().all(Option::is_some),
                "a later unchanged suffix must not forget earlier relational replay"
            );
            let next = candidate.store.as_ref().unwrap();
            next.validate_authoritative_relational_index_open().unwrap();
            let current_reader = next.open_relational_row_snapshot_reader().unwrap().unwrap();
            assert_eq!(
                current_reader.identity().visible_commit_epoch,
                store.commit_epoch()
            );
            for key in ["kept", "new-a", "new-b"] {
                assert_row(&current_reader, key, true);
            }
            assert_eq!(old_reader.identity().visible_commit_epoch, old_epoch);
            assert_row(&old_reader, "kept", true);
            for key in ["new-a", "new-b"] {
                assert_row(&old_reader, key, false);
            }
            let expected = store
                .node_records_owned()
                .collect::<crate::Result<Vec<_>>>()
                .unwrap();
            store
                .publish_checkpoint_candidate(&mut candidate, None, &Default::default())
                .unwrap();
            assert_eq!(
                store
                    .node_records_owned()
                    .collect::<crate::Result<Vec<_>>>()
                    .unwrap(),
                expected
            );
            for key in ["kept", "new-a", "new-b"] {
                assert_row(&current_reader, key, true);
            }
            for key in ["new-a", "new-b"] {
                assert_row(&old_reader, key, false);
            }
            drop(current_reader);
            drop(old_reader);
            drop(candidate);
            drop(pinned);
            drop(store);
            let reopened = GraphStore::open_with_durability_and_replay_config(
                &root,
                &mut catalog,
                policy,
                replay,
            )
            .unwrap();
            assert_eq!(
                reopened
                    .node_records_owned()
                    .collect::<crate::Result<Vec<_>>>()
                    .unwrap(),
                expected
            );
            assert_eq!(reopened.relational_state().total_row_count(), 3);
            reopened
                .validate_authoritative_relational_index_open()
                .unwrap();
            let recovered_reader = reopened
                .open_relational_row_snapshot_reader()
                .unwrap()
                .unwrap();
            for key in ["kept", "new-a", "new-b"] {
                assert_row(&recovered_reader, key, true);
            }
            drop(recovered_reader);
            drop(reopened);
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

#[test]
fn private_relational_prefix_clone_denial_preserves_candidate_for_retry() {
    for mode in [
        RelationalIndexMode::Materialized,
        RelationalIndexMode::Authoritative,
    ] {
        private_relational_prefix_clone_denial_case(mode);
    }
}

fn private_relational_prefix_clone_denial_case(mode: RelationalIndexMode) {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeMemorySnapshot,
        RuntimeResourceBudget, RuntimeResourceSnapshot,
    };
    let root = std::env::temp_dir().join(format!(
        "hawdb-private-prefix-admission-{}",
        hawdb_core::generate_uuidv7().unwrap()
    ));
    let names: Vec<String> = (0..128)
        .map(|i| format!("t{i:03}{}", "x".repeat(1020)))
        .collect();
    let table = names[0].clone();
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &root,
        &mut catalog,
        DurabilityPolicy::SyncOnEveryWrite,
        WalReplayConfig {
            relational_index_mode: if mode == RelationalIndexMode::Authoritative {
                RelationalIndexMode::Shadow
            } else {
                mode
            },
            ..WalReplayConfig::default()
        },
    )
    .unwrap();
    let row = |value: &str| RelationalWrite::Insert {
        table: table.clone(),
        rows: vec![RelationalRow::new(vec![RelationalValue::Text(
            value.into(),
        )])],
        mode: RelationalInsertMode::Error,
    };
    let mut writes: Vec<RelationalWrite> = names
        .iter()
        .map(|name| {
            RelationalWrite::CreateTable(RelationalTableSchema {
                name: name.clone(),
                columns: vec![RelationalColumnSchema {
                    name: "id".into(),
                    scalar_type: RelationalScalarType::Text,
                    nullable: false,
                    default: None,
                }],
                primary_key: vec!["id".into()],
                unique_constraints: Vec::new(),
                foreign_keys: Vec::new(),
                indexes: Vec::new(),
            })
        })
        .collect();
    writes.push(row("kept"));
    store
        .commit_relational_transaction(&mut catalog, RelationalTransaction { writes })
        .unwrap();
    store.checkpoint(&catalog).unwrap();
    drop(store);
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &root,
        &mut catalog,
        DurabilityPolicy::SyncOnEveryWrite,
        WalReplayConfig {
            relational_index_mode: mode,
            ..WalReplayConfig::default()
        },
    )
    .unwrap();
    store
        .create_node(&mut catalog, "Memory", BTreeMap::new())
        .unwrap();
    let pinned = store.checkpoint_source();
    let mut candidate = pinned
        .prepare_checkpoint_candidate(&catalog)
        .unwrap()
        .unwrap();
    store
        .commit_relational_transaction(
            &mut catalog,
            RelationalTransaction {
                writes: vec![row("new")],
            },
        )
        .unwrap();
    candidate.catch_up(&store).unwrap();
    let governor_for = |budget| {
        RuntimeGovernor::new(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(budget),
                background_task_limit: Some(NonZeroUsize::MIN),
                ..RuntimeGovernorConfig::shared_host()
            },
            RuntimeResourceSnapshot::from_parts(
                RuntimeResourceBudget::from_limits(NonZeroUsize::MIN, None, None),
                RuntimeMemorySnapshot::from_limits(Some(1 << 30), Some(1 << 30), None, None, None),
            ),
            IoConcurrencyBudget::new(2, 1),
        )
    };
    let governor = governor_for(4096);
    let admission = governor
        .try_admit_incremental_maintenance(4096, 4096, 1, RuntimeTaskContext::default())
        .unwrap();
    let work =
        crate::background::CheckpointWorkContext::new(admission.task_context().unwrap().clone());
    let before = store.checkpoint_source_identity();
    let observer = crate::test_allocator::AllocationObservation::start();
    let result = candidate.finish_catch_up_with_work_context(&work);
    let large_allocations = observer.finish();
    assert!(matches!(
        result,
        Err(crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        ))
    ));
    assert_eq!(large_allocations, 0, "deny before cloning wide metadata");
    assert_eq!(governor.snapshot().admitted_memory_bytes, 4096);
    assert_eq!(store.checkpoint_source_identity(), before);
    assert!(!candidate.replay_finalized);
    assert!(!candidate.failed);
    assert!(candidate.recovery_selectors.iter().all(Option::is_none));
    // Seal only, without preparing a selector: this is the real intermediate
    // boundary preceding reader admission. Repeating the seal reuses evidence.
    let identity = candidate
        .recovery_source
        .as_ref()
        .unwrap()
        .prefix_identity()
        .unwrap();
    let private = candidate.store.as_mut().unwrap();
    let builder = private
        .relational_row_pages
        .recovery_builder
        .as_mut()
        .unwrap();
    let report = builder
        .seal_private_with_state(
            private.commit_epoch,
            identity,
            None,
            &private.relational_state,
            &crate::background::CheckpointWorkContext::default(),
        )
        .unwrap();
    let cached = builder
        .seal_private_with_state(
            private.commit_epoch,
            identity,
            None,
            &private.relational_state,
            &work,
        )
        .unwrap();
    assert_eq!(cached, report);
    let immutable = root.join(crate::relational::relational_row_delta_prefix_file(
        report.generation.base_generation,
        report.generation.delta_generation,
        report.visible_commit_epoch,
    ));
    let sealed_bytes = std::fs::read(&immutable).unwrap();
    let observer = crate::test_allocator::AllocationObservation::start();
    let result = candidate.finish_catch_up_with_work_context(&work);
    let large_allocations = observer.finish();
    assert!(matches!(
        result,
        Err(crate::background::CheckpointOperationError::Work(
            crate::background::CheckpointWorkError::Memory(_)
        ))
    ));
    assert_eq!(
        large_allocations, 0,
        "deny before reading the wide manifest"
    );
    assert!(!candidate.failed);
    assert!(!candidate.replay_finalized);
    assert!(candidate.recovery_selectors.iter().all(Option::is_none));
    assert_eq!(std::fs::read(&immutable).unwrap(), sealed_bytes);
    assert_eq!(store.checkpoint_source_identity(), before);
    if mode == RelationalIndexMode::Authoritative {
        let private = candidate.store.as_mut().unwrap();
        let builder = private
            .relational_index_shadow
            .recovery_builder
            .as_mut()
            .unwrap();
        let denied = work.classify(|work| {
            builder.seal_private_with_recovery_source(private.commit_epoch, identity, work)
        });
        assert!(
            matches!(
                denied,
                Err(crate::background::CheckpointOperationError::Work(
                    crate::background::CheckpointWorkError::Memory(_)
                ))
            ),
            "index clone/encoding must honor the same undersized work budget"
        );
        assert_eq!(governor.snapshot().admitted_memory_bytes, 4096);
        let index_report = builder
            .seal_private_with_recovery_source(
                private.commit_epoch,
                identity,
                &crate::background::CheckpointWorkContext::default(),
            )
            .unwrap();
        let index_path = root.join(crate::relational::relational_index_recovery_prefix_file(
            index_report.base_generation,
            index_report.delta_generation,
            index_report.recovered_commit_epoch,
        ));
        let index_bytes = std::fs::read(&index_path).unwrap();
        let repeated = builder
            .seal_private_with_recovery_source(private.commit_epoch, identity, &work)
            .unwrap();
        assert_eq!(repeated, index_report);
        assert_eq!(std::fs::read(&index_path).unwrap(), index_bytes);
        assert!(index_bytes.len() > 512);
        let reader_governor = governor_for(512);
        let reader_admission = reader_governor
            .try_admit_incremental_maintenance(512, 512, 1, RuntimeTaskContext::default())
            .unwrap();
        let reader_work = crate::background::CheckpointWorkContext::new(
            reader_admission.task_context().unwrap().clone(),
        );
        let durable = private.durable.as_ref().unwrap();
        let base = private
            .relational_index_shadow
            .read_view
            .as_ref()
            .unwrap()
            .pinned_base();
        let denied = reader_work.classify(|work| {
            crate::relational::RelationalIndexRecoveryReader::open_private_prefix_with_pinned_base(
                &index_path,
                Arc::clone(&base),
                crate::relational::RelationalRecoveryFence::new(private.commit_epoch, identity),
                crate::relational::RelationalIndexRecoveryConfig::default(),
                Arc::clone(&durable.segment_cache),
                durable.store_id(),
                work,
            )
        });
        assert!(
            matches!(
                denied,
                Err(crate::background::CheckpointOperationError::Work(
                    crate::background::CheckpointWorkError::Memory(_)
                ))
            ),
            "an already sealed index still admits its reader buffer before allocation"
        );
        assert_eq!(std::fs::read(&index_path).unwrap(), index_bytes);
        let binding = private
            .relational_index_shadow
            .generation_artifacts
            .unwrap();
        let observer = crate::test_allocator::AllocationObservation::start();
        let denied_base = reader_work.classify(|work| {
            crate::relational::RelationalIndexShadowReader::open_bound_generation_with_work_context(
                durable.root_path(),
                binding,
                crate::relational::RelationalIndexShadowConfig::default(),
                Some(Arc::clone(&durable.segment_cache)),
                durable.store_id(),
                work,
            )
        });
        let large_allocations = observer.finish();
        assert!(
            matches!(
                denied_base,
                Err(crate::background::CheckpointOperationError::Work(
                    crate::background::CheckpointWorkError::Memory(_)
                ))
            ),
            "first index-base open must admit the wide manifest before reading it"
        );
        assert_eq!(large_allocations, 0);
        // Exercise the cold path on the positive retry. Production still uses
        // a pinned base whenever its complete canonical binding matches.
        private.relational_index_shadow.read_view = None;
        drop((reader_work, reader_admission));
        assert_eq!(reader_governor.snapshot().admitted_memory_bytes, 0);
        assert_eq!(store.checkpoint_source_identity(), before);
    }
    drop((work, admission));
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);

    let retry_governor = governor_for(16 * 1024 * 1024);
    let retry = retry_governor
        .try_admit_incremental_maintenance(4096, 16 * 1024 * 1024, 1, RuntimeTaskContext::default())
        .unwrap();
    let retry_work =
        crate::background::CheckpointWorkContext::new(retry.task_context().unwrap().clone());
    candidate
        .finish_catch_up_with_work_context(&retry_work)
        .unwrap();
    assert!(candidate.replay_finalized);
    assert!(!candidate.failed);
    assert_eq!(store.checkpoint_source_identity(), before);
    assert!(
        retry_governor.snapshot().admitted_memory_bytes > 4096,
        "decoded reader memory stays admitted after finalization"
    );
    assert_eq!(std::fs::read(&immutable).unwrap(), sealed_bytes);
    let private = candidate.store.as_ref().unwrap();
    assert_eq!(private.relational_state.row_count(&table), 2);
    assert_eq!(private.relational_state.table_schemas().count(), 128);
    let pinned_index_base = private
        .relational_index_shadow
        .read_view
        .as_ref()
        .map(|view| view.pinned_base());
    assert_eq!(
        pinned_index_base.is_some(),
        mode == RelationalIndexMode::Authoritative
    );
    let reader = private
        .open_relational_row_snapshot_reader()
        .unwrap()
        .unwrap();
    for key in ["kept", "new"] {
        let (row, _) = reader
            .point_projected(
                &table,
                &RelationalKey(vec![RelationalValue::Text(key.into())]),
                &[0],
                RelationalRowPageSnapshotReadLimits::default(),
                &mut RelationalHydrationBudget::default(),
                &RuntimeTaskContext::default(),
            )
            .unwrap();
        assert_eq!(
            row.unwrap().fields[0].value,
            RelationalValue::Text(key.into())
        );
    }
    store
        .commit_relational_transaction(
            &mut catalog,
            RelationalTransaction {
                writes: vec![row("later")],
            },
        )
        .unwrap();
    candidate.catch_up(&store).unwrap();
    candidate
        .finish_catch_up_with_work_context(&retry_work)
        .unwrap();
    let private = candidate.store.as_ref().unwrap();
    assert_eq!(private.relational_state.row_count(&table), 3);
    if let Some(base) = &pinned_index_base {
        assert!(
            Arc::ptr_eq(
                base,
                &private
                    .relational_index_shadow
                    .read_view
                    .as_ref()
                    .unwrap()
                    .pinned_base()
            ),
            "later prefixes share the already validated index base without reloading metadata"
        );
    }
    let (absent, _) = reader
        .point_projected(
            &table,
            &RelationalKey(vec![RelationalValue::Text("later".into())]),
            &[0],
            RelationalRowPageSnapshotReadLimits::default(),
            &mut RelationalHydrationBudget::default(),
            &RuntimeTaskContext::default(),
        )
        .unwrap();
    assert!(
        absent.is_none(),
        "the old snapshot retains its original prefix"
    );
    drop((retry_work, retry, candidate, pinned, store));
    drop(pinned_index_base);
    assert!(
        retry_governor.snapshot().admitted_memory_bytes > 0,
        "a surviving snapshot retains decoded backing after the owner closes"
    );
    drop(reader);
    assert_eq!(retry_governor.snapshot().admitted_memory_bytes, 0);
    std::fs::remove_dir_all(root).unwrap();
}

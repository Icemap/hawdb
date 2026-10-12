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

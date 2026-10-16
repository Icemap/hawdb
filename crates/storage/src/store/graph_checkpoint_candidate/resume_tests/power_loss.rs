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
use crate::power_loss::image::{CrashPlan, ImageLimits, PersistOperation};
use crate::power_loss::{IoEvent, ObservationBoundary, ObservationPoint, PowerLossModel};

fn qualify_resumed_wal(event: IoEvent) {
    let mut captures = 0;
    let mut torn_images = 0;
    let mut rejected_corrupt_images = 0;
    for durability in [
        DurabilityPolicy::SyncOnEveryWrite,
        DurabilityPolicy::SyncOnCheckpoint,
    ] {
        for boundary in [ObservationBoundary::Before, ObservationBoundary::After] {
            let root = std::env::temp_dir().join(format!(
                "hawdb-resumed-wal-power-{}",
                hawdb_core::generate_uuidv7().unwrap()
            ));
            let project = crate::file_descriptors::ProjectFileDescriptors::acquire(
                &root,
                WalReplayConfig::default().max_open_files,
            )
            .unwrap();
            let model = PowerLossModel::attach(&project, ImageLimits::default()).unwrap();
            let mut fixture = Fixture::at(root.clone(), durability, true);
            let governor = governor(4 * 1024 * 1024);
            let permit = governor
                .try_admit(
                    RuntimeWorkRequest::background_maintenance(4 * 1024 * 1024)
                        .with_io_wave_slots(1),
                )
                .unwrap();
            let cut = Arc::new(CutWaves {
                admitted: permit.bind_task_context(RuntimeTaskContext::without_deadline(
                    RuntimeCancellationToken::new(),
                )),
                private_wal_path: fixture.private_wal_path(),
                after: fixture.first_record_end,
                before: fixture.private_wal_end - 1,
                fired: AtomicBool::new(false),
            });
            let task = cut.admitted.clone().with_io_wave_controller(cut.clone());
            fixture
                .candidate
                .catch_up_with_task_context(&fixture.store, &task)
                .unwrap_err();
            assert!(cut.fired.load(AtomicOrdering::SeqCst));
            assert!(!fixture.candidate.failed);
            let partial = fs::metadata(fixture.private_wal_path()).unwrap().len();
            assert!(partial > fixture.first_record_end && partial < fixture.private_wal_end);
            assert_eq!(
                fixture.candidate.candidate_wal_bytes,
                fixture.first_record_end
            );
            let relative = fixture
                .private_wal_path()
                .strip_prefix(&root)
                .unwrap()
                .to_owned();
            model
                .observe(ObservationPoint {
                    event,
                    relative_path: relative.clone(),
                    boundary,
                    skip_matches: 0,
                    include_descendants: false,
                    keep_last: false,
                })
                .unwrap();
            drop(task);
            drop(cut);
            let fresh = permit.bind_task_context(RuntimeTaskContext::default());
            fixture
                .candidate
                .catch_up_with_task_context(&fixture.store, &fresh)
                .unwrap();
            model.capture().unwrap();
            let snapshot = model.take_observation().unwrap().unwrap();
            captures += 1;
            assert_eq!(snapshot.observed_path(), Some(relative.as_path()));
            let complete = snapshot.persist_all_plan();
            let mut plans = vec![
                CrashPlan {
                    persistence: Vec::new(),
                },
                complete.clone(),
            ];
            let mut reverse = complete.clone();
            reverse.persistence.reverse();
            plans.push(reverse);
            for operation in &complete.persistence {
                plans.push(CrashPlan {
                    persistence: vec![operation.clone()],
                });
            }
            let writes = snapshot.uncovered_writes(&relative).unwrap();
            for write in writes {
                assert!(write.length > 1);
                for bytes in [0..1, 0..write.length / 2, write.length / 2..write.length] {
                    let mut plan = complete.clone();
                    let operation = plan
                        .persistence
                        .iter_mut()
                        .find(|operation| **operation == PersistOperation::Whole(write.operation))
                        .unwrap();
                    *operation = PersistOperation::TornWrite {
                        operation: write.operation,
                        bytes,
                    };
                    plans.push(plan);
                }
            }
            for (index, plan) in plans.iter().enumerate() {
                let image = root.with_extension(format!("crash-{index}"));
                snapshot.crash(plan).unwrap().materialize(&image).unwrap();
                let original = fixture.store.durable.as_ref().unwrap();
                let evidence: Vec<_> = [
                    original.wal_path.clone(),
                    original.manifest_path().to_path_buf(),
                ]
                .into_iter()
                .map(|path| {
                    let path = image.join(path.strip_prefix(&root).unwrap());
                    let bytes = fs::read(&path).unwrap();
                    (path, bytes)
                })
                .collect();
                let mut catalog = Catalog::default();
                let recovered = match GraphStore::open_with_durability(
                    &image,
                    &mut catalog,
                    durability,
                ) {
                    Ok(recovered) => recovered,
                    Err(error) => {
                        // Reordered relaxed writes can leave a hole before a
                        // later complete record. Strict recovery must reject
                        // that uncertainty and retain authoritative evidence.
                        assert!(
                            durability == DurabilityPolicy::SyncOnCheckpoint
                                && *plan != complete
                                && matches!(&error, HawDBError::Storage(message)
                                    if message.starts_with("WAL corruption")),
                            "unexpected recovery rejection: {durability:?} {event:?} {boundary:?} {plan:?}: {error}"
                        );
                        for (path, bytes) in evidence {
                            assert_eq!(fs::read(path).unwrap(), bytes);
                        }
                        rejected_corrupt_images += 1;
                        torn_images += usize::from(plan.persistence.iter().any(|operation| {
                            matches!(operation, PersistOperation::TornWrite { .. })
                        }));
                        fs::remove_dir_all(image).unwrap();
                        continue;
                    }
                };
                let mut rows: Vec<_> = recovered
                    .scan_nodes(None)
                    .map(|node| node.properties.clone())
                    .collect();
                rows.sort_by_key(|row| match row["id"] {
                    Value::Int(id) => id,
                    _ => panic!("invalid recovered id"),
                });
                if durability == DurabilityPolicy::SyncOnEveryWrite || *plan == complete {
                    assert_eq!(
                        rows.len(),
                        3,
                        "{durability:?} {event:?} {boundary:?} {plan:?}"
                    );
                }
                assert!((1..=3).contains(&rows.len()), "covered bootstrap was lost");
                for (index, row) in rows.iter().enumerate() {
                    assert_eq!(row["id"], Value::Int(index as i64 + 1));
                    assert_eq!(row["payload"], Value::String("\0界🙂".repeat(30_000)));
                    assert_eq!(row.len(), 2);
                }
                torn_images += usize::from(
                    plan.persistence
                        .iter()
                        .any(|operation| matches!(operation, PersistOperation::TornWrite { .. })),
                );
                drop(recovered);
                fs::remove_dir_all(image).unwrap();
            }
            fixture.unchanged_source();
            drop(fresh);
            drop(permit);
            drop(fixture);
            drop(model);
            drop(project);
            fs::remove_dir_all(root).unwrap();
        }
    }
    assert_eq!(captures, 4);
    assert!(torn_images > 0, "no actual uncovered WAL bytes were torn");
    println!(
        "resumed WAL {event:?}: {captures} physical captures, {torn_images} torn-write images, {rejected_corrupt_images} corrupt images rejected with evidence retained"
    );
}

#[test]
fn resumed_truncate_cuts_preserve_acknowledged_transactions() {
    qualify_resumed_wal(IoEvent::Truncate);
}

#[test]
fn resumed_wal_sync_cuts_preserve_acknowledged_transactions() {
    qualify_resumed_wal(IoEvent::FileSync);
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

fn allocation_payload(root: &Path) -> serde_json::Value {
    let bytes = std::fs::read(root.join("consumer-allocations.hawdb")).unwrap();
    let start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap()
}

#[derive(Clone, Copy)]
enum Cut {
    Written,
    Selected,
    Returned,
}

#[test]
fn consumer_native_settlement_crash_images_preserve_counter_and_only_release_no_effect_debt() {
    let mut images = 0_usize;
    let mut torn = 0;
    for durability in [
        DurabilityPolicy::SyncOnEveryWrite,
        DurabilityPolicy::SyncOnCheckpoint,
    ] {
        for stage in [Cut::Written, Cut::Selected, Cut::Returned] {
            let mut fixture = Fixture::new();
            let source = fixture.root.join("source");
            let mut database = open(&source, durability);
            database
                .query("CREATE (:Memory {id: 'settlement-source'})")
                .unwrap();
            database
                .runtime
                .get_mut()
                .unwrap()
                .store
                .set_search_projection_database_identity(hawdb_core::generate_uuidv7().unwrap());
            database.checkpoint().unwrap();
            let allocation = database
                .runtime
                .get()
                .unwrap()
                .store
                .reserve_search_consumer_allocation(
                    "settlement-crash",
                    &fixture.root.join("target"),
                )
                .unwrap();
            let mut pending = allocation
                .prepare_stationary_generation(&fixture.root)
                .unwrap();
            let generation = pending.generation_name().to_owned();
            fixture
                .model
                .observe(ObservationPoint {
                    event: if matches!(stage, Cut::Written) {
                        IoEvent::Write
                    } else {
                        IoEvent::Rename
                    },
                    relative_path: PathBuf::from("source"),
                    boundary: ObservationBoundary::After,
                    skip_matches: 0,
                    include_descendants: true,
                    keep_last: true,
                })
                .unwrap();
            pending.settle_no_effect().unwrap();
            let returned = fixture.model.capture().unwrap();
            let observed = fixture
                .model
                .take_observation()
                .unwrap()
                .expect("actual settlement I/O cut");
            let observed_path = observed.observed_path().unwrap().to_path_buf();
            assert_eq!(observed_path.parent(), Some(Path::new("source")));
            assert!(observed_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("consumer-settlement-"));
            let cut = if matches!(stage, Cut::Returned) {
                returned
            } else {
                observed
            };
            let mut plans = publication_fault_plans(&cut);
            if matches!(stage, Cut::Written) {
                let writes = cut.uncovered_writes(&observed_path).unwrap();
                assert_eq!(writes.len(), 1);
                let write = writes[0];
                assert!(write.length > 32);
                for range in [
                    0..1,
                    0..32,
                    0..write.length / 2,
                    1..write.length,
                    write.length - 4..write.length,
                    1..write.length - 1,
                ] {
                    for reversed in [false, true] {
                        let mut plan = cut.persist_all_plan();
                        plan.persistence.retain(|op| !matches!(op, PersistOperation::Whole(id) if *id == write.operation));
                        plan.persistence.push(PersistOperation::TornWrite {
                            operation: write.operation,
                            bytes: range.clone(),
                        });
                        if reversed {
                            plan.persistence.reverse();
                        }
                        plans.push(plan);
                        torn += 1;
                    }
                }
            }
            drop((pending, database));
            for plan in plans {
                let image = fixture.image(&cut, &plan);
                let source = image.join("source");
                let mut reopened = open(&source, durability);
                assert_eq!(
                    reopened
                        .query("MATCH (m:Memory {id: 'settlement-source'}) RETURN m.id")
                        .unwrap()
                        .rows
                        .len(),
                    1
                );
                reopened
                    .runtime
                    .get()
                    .unwrap()
                    .store
                    .verify_search_consumer_allocation_evidence()
                    .unwrap();
                let payload = allocation_payload(&source);
                assert_eq!(payload["high_water"], 3);
                let retained = payload["pending"].as_array().unwrap().len();
                match stage {
                    Cut::Written => assert_eq!(retained, 1),
                    Cut::Selected => assert!(retained <= 1),
                    Cut::Returned => assert_eq!(retained, 0),
                }
                if retained == 1 {
                    assert_eq!(payload["pending"][0]["native_birth"]["state"], "Pending");
                }
                assert!(!image.join(&generation).exists());
                assert!(reopened
                    .runtime
                    .get()
                    .unwrap()
                    .projection_consumers
                    .records
                    .is_empty());
                let next = reopened
                    .runtime
                    .get()
                    .unwrap()
                    .store
                    .reserve_search_consumer_allocation(
                        "settlement-retry",
                        &image.join("next-target"),
                    )
                    .unwrap();
                assert_eq!(allocation_payload(&source)["high_water"], 6);
                drop(next);
                images += 1;
                if images.is_multiple_of(25) {
                    eprintln!("consumer-settlement crash images reopened: {images}");
                }
            }
        }
    }
    assert!(images >= 12);
    assert_eq!(torn, 24);
    eprintln!("consumer-settlement-power-loss: images={images}, torn={torn}, durability-modes=2");
}

#[test]
fn consumer_native_settlement_lost_response_retries_actual_absent_record_barriers() {
    use hawdb_storage::file_io::SourceDirectory;
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut fixture = Fixture::new();
    let source = fixture.root.join("source");
    let mut database = open(&source, DurabilityPolicy::SyncOnEveryWrite);
    database
        .query("CREATE (:Memory {id: 'settlement-reply'})")
        .unwrap();
    database
        .runtime
        .get_mut()
        .unwrap()
        .store
        .set_search_projection_database_identity(hawdb_core::generate_uuidv7().unwrap());
    database.checkpoint().unwrap();
    let allocation = database
        .runtime
        .get()
        .unwrap()
        .store
        .reserve_search_consumer_allocation("lost-settlement", &fixture.root.join("target"))
        .unwrap();
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.root)
        .unwrap();
    let context = database
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let capture = Rc::new(SourceDirectory::capture(&fixture.root, &context).unwrap());
    let blockers = Rc::new(RefCell::new(Vec::new()));
    let callback_capture = Rc::clone(&capture);
    let callback_blockers = Rc::clone(&blockers);
    let callback_context = context.clone();
    let error = SourceDirectory::observe_next_control_rename(
        move || loop {
            match callback_capture.duplicate(&callback_context) {
                Ok(blocker) => callback_blockers.borrow_mut().push(blocker),
                Err(error) => {
                    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
                    break;
                }
            }
        },
        || pending.settle_no_effect(),
    )
    .unwrap()
    .unwrap_err();
    assert!(!blockers.borrow().is_empty());
    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
    assert!(allocation_payload(&source)["pending"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(pending.create().is_err());
    let before = fixture.model.capture().unwrap();
    blockers.borrow_mut().clear();
    {
        let image = fixture.image(&before, &CrashPlan::default());
        let recovered = open(&image.join("source"), DurabilityPolicy::SyncOnEveryWrite);
        recovered
            .runtime
            .get()
            .unwrap()
            .store
            .verify_search_consumer_allocation_evidence()
            .unwrap();
        assert_eq!(
            allocation_payload(&image.join("source"))["pending"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
    fixture
        .model
        .observe(ObservationPoint {
            event: IoEvent::FileSync,
            relative_path: PathBuf::from("source/consumer-allocations.hawdb"),
            boundary: ObservationBoundary::After,
            skip_matches: 0,
            include_descendants: false,
            keep_last: false,
        })
        .unwrap();
    pending.settle_no_effect().unwrap();
    assert!(fixture.model.take_observation().unwrap().is_some());
    let after = fixture.model.capture().unwrap();
    {
        let image = fixture.image(&after, &CrashPlan::default());
        let recovered = open(&image.join("source"), DurabilityPolicy::SyncOnEveryWrite);
        recovered
            .runtime
            .get()
            .unwrap()
            .store
            .verify_search_consumer_allocation_evidence()
            .unwrap();
        let ledger = allocation_payload(&image.join("source"));
        assert!(ledger["pending"].as_array().unwrap().is_empty());
        assert_eq!(ledger["high_water"], 3);
    }
    let stable = allocation_payload(&source);
    pending.settle_no_effect().unwrap();
    assert_eq!(allocation_payload(&source), stable);
}

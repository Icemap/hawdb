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
    IntentWritten,
    IntentSelected,
    MarkerRemoved,
    DirectoryRemoved,
    TombstoneSelected,
    Returned,
}

#[test]
fn consumer_native_retirement_crash_images_keep_debt_until_original_deletion_is_durable() {
    let mut images = 0_usize;
    let mut torn = 0;
    for durability in [
        DurabilityPolicy::SyncOnEveryWrite,
        DurabilityPolicy::SyncOnCheckpoint,
    ] {
        for stage in [
            Cut::IntentWritten,
            Cut::IntentSelected,
            Cut::MarkerRemoved,
            Cut::DirectoryRemoved,
            Cut::TombstoneSelected,
            Cut::Returned,
        ] {
            let mut fixture = Fixture::new();
            let source = fixture.root.join("source");
            let mut database = open(&source, durability);
            database
                .query("CREATE (:Memory {id: 'retirement-source'})")
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
                    "retirement-crash",
                    &fixture.root.join("target"),
                )
                .unwrap();
            let mut pending = allocation
                .prepare_stationary_generation(&fixture.root)
                .unwrap();
            pending.create().unwrap();
            let generation = pending.generation_name().to_owned();
            let (event, relative_path, skip_matches, include_descendants) = match stage {
                Cut::IntentWritten => (IoEvent::Write, PathBuf::from("source"), 0, true),
                Cut::MarkerRemoved => (
                    IoEvent::Remove,
                    PathBuf::from(&generation).join("consumer-birth.hawdb"),
                    0,
                    false,
                ),
                Cut::DirectoryRemoved => (IoEvent::Remove, PathBuf::from(&generation), 0, false),
                Cut::TombstoneSelected => (IoEvent::Rename, PathBuf::from("source"), 1, true),
                _ => (IoEvent::Rename, PathBuf::from("source"), 0, true),
            };
            fixture
                .model
                .observe(ObservationPoint {
                    event,
                    relative_path,
                    skip_matches,
                    include_descendants,
                    boundary: ObservationBoundary::After,
                    keep_last: false,
                })
                .unwrap();
            pending.retire_unpublished().unwrap();
            let returned = fixture.model.capture().unwrap();
            let observed = fixture
                .model
                .take_observation()
                .unwrap()
                .expect("actual retirement I/O cut");
            let observed_path = observed.observed_path().unwrap().to_path_buf();
            let cut = if matches!(stage, Cut::Returned) {
                returned
            } else {
                observed
            };
            let mut plans = publication_fault_plans(&cut);
            if matches!(stage, Cut::IntentWritten) {
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
                        .query("MATCH (m:Memory {id: 'retirement-source'}) RETURN m.id")
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
                let rows = payload["pending"].as_array().unwrap();
                assert!(rows.len() <= 1);
                let state = rows
                    .first()
                    .map(|row| row["native_birth"]["state"].as_str().unwrap());
                let target = image.join(&generation);
                match state {
                    Some("Recorded") => assert!(target.join("consumer-birth.hawdb").is_file()),
                    Some("Retiring") => {}
                    Some("Retired") | None => assert!(!target.exists()),
                    other => panic!("unexpected retirement state {other:?}"),
                }
                if matches!(stage, Cut::IntentWritten) {
                    assert_eq!(state, Some("Recorded"));
                }
                if matches!(stage, Cut::MarkerRemoved | Cut::DirectoryRemoved) {
                    assert_eq!(state, Some("Retiring"));
                }
                if matches!(stage, Cut::TombstoneSelected) {
                    assert!(!target.exists());
                }
                if matches!(stage, Cut::Returned) {
                    assert!(rows.is_empty());
                }
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
                        "retirement-next",
                        &image.join("next-target"),
                    )
                    .unwrap();
                assert_eq!(allocation_payload(&source)["high_water"], 6);
                drop(next);
                images += 1;
                if images.is_multiple_of(25) {
                    eprintln!("consumer-retirement crash images reopened: {images}");
                }
            }
        }
    }
    assert!(images >= 24);
    assert_eq!(torn, 24);
    eprintln!("consumer-retirement-power-loss: images={images}, torn={torn}, durability-modes=2");
}

#[test]
fn consumer_native_retirement_lost_intent_response_repeats_source_barrier_before_deletion() {
    use hawdb_storage::file_io::SourceDirectory;
    use std::cell::RefCell;
    use std::rc::Rc;

    let mut fixture = Fixture::new();
    let source = fixture.root.join("source");
    let mut database = open(&source, DurabilityPolicy::SyncOnEveryWrite);
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
        .reserve_search_consumer_allocation("retirement-lost-intent", &fixture.root.join("target"))
        .unwrap();
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.root)
        .unwrap();
    pending.create().unwrap();
    let generation = pending.generation_name().to_owned();
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
                Ok(fd) => callback_blockers.borrow_mut().push(fd),
                Err(error) => {
                    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
                    break;
                }
            }
        },
        || pending.retire_unpublished(),
    )
    .unwrap()
    .unwrap_err();
    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
    assert!(!blockers.borrow().is_empty());
    assert_eq!(
        allocation_payload(&source)["pending"][0]["native_birth"]["state"],
        "Retiring"
    );
    assert!(fixture
        .root
        .join(&generation)
        .join("consumer-birth.hawdb")
        .exists());
    assert!(pending.born_directory().is_err());
    let uncertain = fixture.model.capture().unwrap();
    blockers.borrow_mut().clear();
    {
        let image = fixture.image(&uncertain, &CrashPlan::default());
        let recovered = open(&image.join("source"), DurabilityPolicy::SyncOnEveryWrite);
        recovered
            .runtime
            .get()
            .unwrap()
            .store
            .verify_search_consumer_allocation_evidence()
            .unwrap();
        assert_eq!(
            allocation_payload(&image.join("source"))["pending"][0]["native_birth"]["state"],
            "Recorded"
        );
        assert!(image
            .join(&generation)
            .join("consumer-birth.hawdb")
            .exists());
    }
    fixture
        .model
        .observe(ObservationPoint {
            event: IoEvent::DirectorySync,
            relative_path: PathBuf::from("source"),
            boundary: ObservationBoundary::After,
            skip_matches: 0,
            include_descendants: false,
            keep_last: false,
        })
        .unwrap();
    pending.retire_unpublished().unwrap();
    let intent = fixture
        .model
        .take_observation()
        .unwrap()
        .expect("actual retry source-root barrier");
    {
        let image = fixture.image(&intent, &CrashPlan::default());
        let recovered = open(&image.join("source"), DurabilityPolicy::SyncOnEveryWrite);
        recovered
            .runtime
            .get()
            .unwrap()
            .store
            .verify_search_consumer_allocation_evidence()
            .unwrap();
        assert_eq!(
            allocation_payload(&image.join("source"))["pending"][0]["native_birth"]["state"],
            "Retiring"
        );
        assert!(image
            .join(&generation)
            .join("consumer-birth.hawdb")
            .exists());
    }
    let completed = fixture.model.capture().unwrap();
    let image = fixture.image(&completed, &CrashPlan::default());
    let recovered = open(&image.join("source"), DurabilityPolicy::SyncOnEveryWrite);
    recovered
        .runtime
        .get()
        .unwrap()
        .store
        .verify_search_consumer_allocation_evidence()
        .unwrap();
    assert!(allocation_payload(&image.join("source"))["pending"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(allocation_payload(&image.join("source"))["high_water"], 3);
    assert!(!image.join(&generation).exists());
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::{SearchProjectionConsumerId, SearchProjectionConsumerOptions};
use std::num::NonZeroU64;

const LEDGER: &str = "consumer-allocations.hawdb";

fn allocation_counter(database: &Database, root: &Path) -> u64 {
    database
        .runtime
        .get()
        .unwrap()
        .store
        .verify_search_consumer_allocation_evidence()
        .unwrap();
    let bytes = std::fs::read(root.join(LEDGER)).unwrap();
    let start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    let payload: serde_json::Value =
        serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap();
    payload["high_water"].as_u64().unwrap()
}

fn reserve_then_fail(database: &mut Database, destination: &Path) {
    let error = database
        .create_search_projection_consumer(
            SearchProjectionConsumerId::new("crash-reservation").unwrap(),
            destination,
            SearchProjectionConsumerOptions::new(NonZeroU64::new(100).unwrap()),
            |_, _| Err(HawDBError::Execution("allocation-cut callback".into())),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("allocation-cut callback"),
        "{error}"
    );
    assert!(!destination.exists());
}

#[derive(Clone, Copy, Debug)]
enum Cut {
    Unsealed,
    Selected,
    ReturnedReservation,
}

#[test]
fn consumer_allocation_crash_images_preserve_reservation_barriers_and_nonreuse() {
    let mut images = 0_usize;
    let mut torn = 0;
    for durability in [
        DurabilityPolicy::SyncOnEveryWrite,
        DurabilityPolicy::SyncOnCheckpoint,
    ] {
        for stage in [Cut::Unsealed, Cut::Selected, Cut::ReturnedReservation] {
            let mut fixture = Fixture::new();
            let source = fixture.root.join("source");
            let mut database = open(&source, durability);
            database
                .query("CREATE (:Memory {id: 'allocation-source'})")
                .unwrap();
            database.checkpoint().unwrap();
            assert_eq!(allocation_counter(&database, &source), 0);
            fixture
                .model
                .observe(ObservationPoint {
                    event: if matches!(stage, Cut::Unsealed) {
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
            reserve_then_fail(
                &mut database,
                &fixture.root.with_extension("unused-consumer"),
            );
            assert_eq!(allocation_counter(&database, &source), 3);
            let returned_reservation = fixture.model.capture().unwrap();
            let observed = fixture
                .model
                .take_observation()
                .unwrap()
                .expect("actual allocation IO cut");
            let observed_path = observed.observed_path().unwrap().to_path_buf();
            assert!(
                observed_path.parent() == Some(Path::new("source"))
                    && observed_path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .starts_with("consumer-allocation-"),
                "{observed_path:?}"
            );
            let cut = if matches!(stage, Cut::ReturnedReservation) {
                returned_reservation
            } else {
                observed
            };
            let mut plans = publication_fault_plans(&cut);
            if matches!(stage, Cut::Unsealed) {
                let writes = cut.uncovered_writes(&observed_path).unwrap();
                assert_eq!(writes.len(), 1);
                let write = writes[0];
                assert!(write.length > 32);
                // Prefix, suffix and interior tears cross the header, payload
                // and checksum. Apply both orders of pending namespace effects.
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
            drop(database);
            for plan in plans {
                let image = fixture.image(&cut, &plan);
                let source = image.join("source");
                let mut reopened = open(&source, durability);
                assert_eq!(
                    reopened
                        .query("MATCH (m:Memory {id: 'allocation-source'}) RETURN m.id")
                        .unwrap()
                        .rows
                        .len(),
                    1
                );
                let before = allocation_counter(&reopened, &source);
                match stage {
                    Cut::Unsealed => assert_eq!(before, 0),
                    Cut::Selected => assert!(before == 0 || before == 3, "{before}"),
                    Cut::ReturnedReservation => assert_eq!(before, 3),
                }
                // Recovery/nonreuse needs the actual kernel reservation, not
                // another graph checkpoint and empty Search initialization.
                let retry = reopened
                    .runtime
                    .get()
                    .unwrap()
                    .store
                    .reserve_search_consumer_allocation(
                        "crash-retry",
                        &image.with_extension("unused-consumer"),
                    )
                    .unwrap();
                assert_eq!(allocation_counter(&reopened, &source), before + 3);
                drop(retry);
                images += 1;
                if images.is_multiple_of(25) {
                    eprintln!("consumer-allocation crash images reopened: {images}");
                }
            }
        }
    }
    assert!(images >= 30);
    assert_eq!(torn, 24);
    eprintln!("consumer-allocation-power-loss: images={images}, torn={torn}, durability-modes=2");
}

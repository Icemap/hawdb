// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

fn allocation_payload(root: &Path) -> serde_json::Value {
    let bytes = std::fs::read(root.join("consumer-allocations.hawdb")).unwrap();
    let start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap()
}

#[derive(Clone, Copy, Debug)]
enum BirthCut {
    Prepared,
    Created,
    MarkerWritten,
    Returned,
}

#[test]
fn consumer_native_birth_crash_images_require_complete_target_before_recorded_receipt() {
    let mut images = 0_usize;
    let mut torn = 0;
    for durability in [
        DurabilityPolicy::SyncOnEveryWrite,
        DurabilityPolicy::SyncOnCheckpoint,
    ] {
        for stage in [
            BirthCut::Prepared,
            BirthCut::Created,
            BirthCut::MarkerWritten,
            BirthCut::Returned,
        ] {
            let mut fixture = Fixture::new();
            let source = fixture.root.join("source");
            let mut database = open(&source, durability);
            database
                .query("CREATE (:Memory {id: 'birth-source'})")
                .unwrap();
            database
                .runtime
                .get_mut()
                .unwrap()
                .store
                .set_search_projection_database_identity(hawdb_core::generate_uuidv7().unwrap());
            database.checkpoint().unwrap();
            let parent = fixture.root.join("target-parent");
            let context = database
                .runtime
                .get()
                .unwrap()
                .store
                .file_descriptor_context()
                .unwrap();
            let setup =
                hawdb_storage::file_io::SourceDirectory::capture(&fixture.root, &context).unwrap();
            setup
                .create_directory_new(&context, "target-parent")
                .unwrap();
            setup.sync(&context).unwrap();
            drop(setup);
            let allocation = database
                .runtime
                .get()
                .unwrap()
                .store
                .reserve_search_consumer_allocation("birth-crash", &parent)
                .unwrap();
            let mut pending = allocation.prepare_stationary_generation(&parent).unwrap();
            let generation = pending.generation_name().to_owned();
            if matches!(stage, BirthCut::Created | BirthCut::MarkerWritten) {
                fixture
                    .model
                    .observe(ObservationPoint {
                        event: if matches!(stage, BirthCut::Created) {
                            IoEvent::CreateDirectory
                        } else {
                            IoEvent::Write
                        },
                        relative_path: if matches!(stage, BirthCut::Created) {
                            PathBuf::from("target-parent").join(&generation)
                        } else {
                            PathBuf::from("target-parent")
                                .join(&generation)
                                .join("consumer-birth.hawdb")
                        },
                        boundary: ObservationBoundary::After,
                        skip_matches: 0,
                        include_descendants: false,
                        keep_last: false,
                    })
                    .unwrap();
            }
            let cut = if matches!(stage, BirthCut::Prepared) {
                fixture.model.capture().unwrap()
            } else {
                pending.create().unwrap();
                if matches!(stage, BirthCut::Returned) {
                    fixture.model.capture().unwrap()
                } else {
                    fixture
                        .model
                        .take_observation()
                        .unwrap()
                        .expect("actual native birth cut")
                }
            };
            let mut plans = publication_fault_plans(&cut);
            if matches!(stage, BirthCut::MarkerWritten) {
                let path = cut.observed_path().unwrap();
                let writes = cut.uncovered_writes(path).unwrap();
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
            drop(pending);
            drop(database);
            for plan in plans {
                let image = fixture.image(&cut, &plan);
                let source = image.join("source");
                let mut reopened = open(&source, durability);
                assert_eq!(
                    reopened
                        .query("MATCH (m:Memory {id: 'birth-source'}) RETURN m.id")
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
                let row = &payload["pending"][0];
                let state = row["native_birth"]["state"].as_str().unwrap();
                if matches!(stage, BirthCut::Returned) {
                    assert_eq!(state, "Recorded");
                    let marker_path = image
                        .join("target-parent")
                        .join(&generation)
                        .join("consumer-birth.hawdb");
                    let marker = std::fs::read(marker_path).unwrap();
                    let start = marker.iter().position(|b| *b == b'\n').unwrap() + 1;
                    assert_eq!(&marker[..start], b"HAWDB_CONSUMER_BIRTH_V1\n");
                    let checksum_at = marker.len() - 4;
                    assert_eq!(
                        hawdb_integrity::crc32c(&marker[..checksum_at]).get(),
                        u32::from_le_bytes(marker[checksum_at..].try_into().unwrap())
                    );
                    let binding: serde_json::Value =
                        serde_json::from_slice(&marker[start..checksum_at]).unwrap();
                    assert_eq!(binding[0], payload["project_id"]);
                    let mut expected = row.clone();
                    expected["native_birth"]["state"] = "Pending".into();
                    assert_eq!(binding[1], expected);
                } else {
                    assert_eq!(state, "Pending");
                }
                // Recovery observes debt but does not manufacture a producer
                // token from a matching path/marker or publish a Consumer.
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
                    .reserve_search_consumer_allocation("birth-retry", &image.join("next-target"))
                    .unwrap();
                assert_eq!(allocation_payload(&source)["high_water"], 6);
                drop(next);
                images += 1;
                if images.is_multiple_of(25) {
                    eprintln!("consumer-native-birth crash images reopened: {images}");
                }
            }
        }
    }
    assert!(images >= 16);
    assert_eq!(torn, 24);
    eprintln!("consumer-native-birth-power-loss: images={images}, torn={torn}, durability-modes=2");
}

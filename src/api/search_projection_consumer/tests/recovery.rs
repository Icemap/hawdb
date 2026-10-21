// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
#[cfg(feature = "test-support")]
use std::cell::RefCell;
#[cfg(feature = "test-support")]
use std::rc::Rc;

fn ledger(root: &Path) -> serde_json::Value {
    let bytes = std::fs::read(root.join("consumer-allocations.hawdb")).unwrap();
    let start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap()
}

#[test]
fn registered_stationary_creation_retries_original_owner_after_lost_preparation_or_reply() {
    for stage in [
        PublicationStage::AfterCheckpoint,
        PublicationStage::BeforeSelector,
        PublicationStage::AfterSelector,
        PublicationStage::AfterSelectorBarrier,
    ] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let path = fixture.0.join("projection");
        let epoch = db.commit_epoch().unwrap();
        PUBLICATION_FAILURE.set(Some((stage, false)));
        assert!(db
            .create_search_projection_consumer(id("original"), &path, options(100), initialize)
            .is_err());
        let before = ledger(&fixture.0.join("database"));
        assert_eq!(
            db.runtime
                .get()
                .unwrap()
                .projection_consumers
                .recovery
                .len(),
            1
        );
        if matches!(
            stage,
            PublicationStage::AfterSelector | PublicationStage::AfterSelectorBarrier
        ) {
            append(&mut db, 2);
        }
        let consumer = db
            .open_or_create_search_projection_consumer(
                id("original"),
                &path,
                options(999),
                |_, _| panic!("a retained prepared owner must never rerun the initializer"),
            )
            .unwrap();
        assert_eq!(consumer.projection().receipt().source_epoch, epoch);
        assert_eq!(consumer.search_index().unwrap().document_count(), 1);
        let after = ledger(&fixture.0.join("database"));
        assert_eq!(before["high_water"], after["high_water"]);
        assert_eq!(
            before["pending"][0]["operation"],
            after["pending"][0]["operation"]
        );
        assert_eq!(
            db.search_projection_consumer_status(consumer.id())
                .unwrap()
                .expires_at_commit_epoch,
            epoch + 100
        );
        assert!(db
            .runtime
            .get()
            .unwrap()
            .projection_consumers
            .recovery
            .is_empty());
    }
}

#[test]
fn registered_stationary_late_generation_and_resident_refusals_retire_birth_before_retry() {
    for resident in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let path = fixture.0.join("projection");
        let mut limits = RegisteredConsumerLimits::default();
        if resident {
            limits.resident_bytes = NonZeroU64::MIN;
        } else {
            limits.generation_bytes = NonZeroU64::MIN;
        }
        CREATION_LIMITS.set(Some(limits));
        assert!(db
            .create_search_projection_consumer(id("bounds"), &path, options(100), initialize)
            .is_err());
        assert!(
            !path.exists(),
            "late refusal must retire the original unsealed target"
        );
        assert!(ledger(&fixture.0.join("database"))["pending"]
            .as_array()
            .unwrap()
            .is_empty());
        let consumer = db
            .create_search_projection_consumer(id("bounds"), &path, options(100), initialize)
            .unwrap();
        assert_eq!(consumer.search_index().unwrap().document_count(), 1);
    }
}

#[test]
fn registered_stationary_initializer_cleanup_failure_retains_owner_and_foreign_evidence() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let path = fixture.0.join("projection");
    let foreign = path.join("not-produced-by-this-attempt");
    let error = db
        .create_search_projection_consumer(id("cleanup"), &path, options(100), |_, _| {
            std::fs::write(&foreign, b"preserve unknown evidence").unwrap();
            Err(HawDBError::Execution("initializer interrupted".into()))
        })
        .unwrap_err();
    assert!(matches!(error, Error::Database(HawDBError::Execution(_))));
    assert_eq!(
        std::fs::read(&foreign).unwrap(),
        b"preserve unknown evidence"
    );
    assert_eq!(
        db.runtime
            .get()
            .unwrap()
            .projection_consumers
            .recovery
            .len(),
        1
    );
    // Only the test that introduced this unrelated child authorizes its removal.
    std::fs::remove_file(&foreign).unwrap();
    let consumer = db
        .open_or_create_search_projection_consumer(id("cleanup"), &path, options(100), initialize)
        .unwrap();
    assert_eq!(consumer.search_index().unwrap().document_count(), 1);
    assert_eq!(
        ledger(&fixture.0.join("database"))["pending"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn registered_stationary_interrupted_initializer_retires_private_state_and_retries() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let path = fixture.0.join("projection");
    let epoch = db.commit_epoch().unwrap();
    assert!(
        matches!(db.create_search_projection_consumer(id("interrupted"), &path, options(100), |snapshot, index| {
        initialize(snapshot, index)?;
        panic!("initializer interrupted after populating private state");
    }), Err(Error::Database(HawDBError::Execution(message))) if message.contains("initializer panicked"))
    );
    assert!(!path.exists());
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert!(ledger(&fixture.0.join("database"))["pending"]
        .as_array()
        .unwrap()
        .is_empty());
    let consumer = db
        .create_search_projection_consumer(id("interrupted"), &path, options(100), initialize)
        .unwrap();
    assert_eq!(consumer.search_index().unwrap().document_count(), 1);
}

#[test]
fn registered_stationary_removal_settles_failed_creation_without_resurrection() {
    for stage in [
        PublicationStage::BeforeSelector,
        PublicationStage::AfterSelector,
    ] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let old = fixture.0.join("old-projection");
        PUBLICATION_FAILURE.set(Some((stage, false)));
        assert!(db
            .create_search_projection_consumer(id("remove-pending"), &old, options(100), initialize)
            .is_err());
        let before = ledger(&fixture.0.join("database"));
        append(&mut db, 2);
        db.unregister_search_projection_consumer(&id("remove-pending"))
            .unwrap();
        db.unregister_search_projection_consumer(&id("remove-pending"))
            .unwrap();
        assert!(db
            .runtime
            .get()
            .unwrap()
            .projection_consumers
            .recovery
            .is_empty());
        assert!(
            old.exists(),
            "revocation must retain immutable generation evidence"
        );
        let fresh = db
            .open_or_create_search_projection_consumer(
                id("remove-pending"),
                fixture.0.join("fresh-projection"),
                options(100),
                initialize,
            )
            .unwrap();
        assert_ne!(
            before["pending"][0]["registration_uuid"],
            fresh
                .projection()
                .receipt()
                .binding
                .registration_uuid
                .to_string()
        );
        assert_eq!(fresh.search_index().unwrap().document_count(), 2);
    }
}

#[test]
fn registered_stationary_metadata_reservation_reuses_identity_after_lost_reply() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.runtime
        .get_mut()
        .unwrap()
        .store
        .set_search_projection_database_identity(generate_uuidv7().unwrap());
    db.checkpoint().unwrap();
    let target = fixture.0.join("projection");
    let reserve = || {
        db.runtime
            .get()
            .unwrap()
            .store
            .reserve_registered_search_consumer(
                "metadata-reply",
                &target,
                hawdb_storage::consumer_allocation::ConsumerLeasePolicy {
                    max_idle_commits: 100,
                    expires_at_commit_epoch: 100,
                },
            )
            .unwrap()
    };
    let original = reserve();
    let identity = (
        original.registration_uuid(),
        original.projection_uuid(),
        original.checkpoint_uuid(),
    );
    drop(original);
    let before = ledger(&fixture.0.join("database"));
    let retry = reserve();
    assert_eq!(
        (
            retry.registration_uuid(),
            retry.projection_uuid(),
            retry.checkpoint_uuid()
        ),
        identity
    );
    assert_eq!(before, ledger(&fixture.0.join("database")));
    assert!(!target.exists());
    db.runtime
        .get()
        .unwrap()
        .store
        .forget_search_consumer_selection("metadata-reply")
        .unwrap();
    assert!(retry.begin_initial_generation(&fixture.0).is_err());
    let fresh = reserve();
    assert_ne!(fresh.registration_uuid(), identity.0);
    assert_ne!(fresh.checkpoint_uuid(), identity.2);
    assert!(!target.exists());
}

#[cfg(feature = "test-support")]
type DescriptorBlockers = Rc<RefCell<Vec<fs::File>>>;

#[cfg(feature = "test-support")]
fn with_ledger_descriptor_refusal<T>(
    db: &mut Database,
    root: &Path,
    predicate: impl Fn(&serde_json::Value) -> bool + 'static,
    operation: impl FnOnce(&mut Database) -> T,
) -> (T, DescriptorBlockers) {
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let held = Rc::new(RefCell::new(Vec::new()));
    let blockers = Rc::clone(&held);
    let observed_root = root.to_owned();
    let result = fs::SourceDirectory::observe_control_renames(
        move || {
            if !blockers.borrow().is_empty() {
                return;
            }
            if !ledger(&observed_root)["pending"]
                .as_array()
                .unwrap()
                .iter()
                .any(&predicate)
            {
                return;
            }
            loop {
                match context.open(
                    fs::OpenOptions::new().read(true),
                    &observed_root.join("consumer-allocations.hawdb"),
                ) {
                    Ok(file) => blockers.borrow_mut().push(file),
                    Err(error) => {
                        assert!(matches!(
                            HawDBError::from(error),
                            HawDBError::FileDescriptors(_)
                        ));
                        break;
                    }
                }
            }
        },
        || operation(db),
    )
    .unwrap();
    assert!(
        !held.borrow().is_empty(),
        "the requested native boundary must execute"
    );
    (result, held)
}

#[cfg(feature = "test-support")]
#[test]
fn registered_stationary_post_intent_descriptor_refusal_retains_same_prepared_operation() {
    for after_intent in [false, true] {
        let fixture = Fixture::new();
        let root = fixture.0.join("database");
        let mut db = Database::open_with_config(
            &root,
            crate::DatabaseConfig {
                max_open_files: 128,
                ..Default::default()
            },
        )
        .unwrap();
        append(&mut db, 1);
        let path = fixture.0.join("projection");
        let (result, held) = with_ledger_descriptor_refusal(
            &mut db,
            &root,
            move |row| {
                row["candidate_seal"].is_object()
                    && row["publication_intent"].is_object() == after_intent
            },
            |db| {
                db.create_search_projection_consumer(
                    id("post-intent"),
                    &path,
                    options(100),
                    initialize,
                )
            },
        );
        assert!(matches!(
            result,
            Err(Error::Database(HawDBError::FileDescriptors(_)))
        ));
        let before = ledger(&root);
        held.borrow_mut().clear();
        let consumer = db
            .open_or_create_search_projection_consumer(
                id("post-intent"),
                &path,
                options(100),
                |_, _| panic!("prepared generation must survive transient refusal"),
            )
            .unwrap();
        assert_eq!(consumer.search_index().unwrap().document_count(), 1);
        assert_eq!(before["high_water"], ledger(&root)["high_water"]);
        assert_eq!(db.file_descriptor_metrics().unwrap().reserved, 0);
    }
}

#[cfg(feature = "test-support")]
#[test]
fn registered_stationary_lost_reservation_reply_reuses_durable_identity_after_source_advances() {
    let fixture = Fixture::new();
    let root = fixture.0.join("database");
    let mut db = Database::open_with_config(
        &root,
        crate::DatabaseConfig {
            max_open_files: 128,
            ..Default::default()
        },
    )
    .unwrap();
    append(&mut db, 1);
    let path = fixture.0.join("projection");
    let observed_root = root.clone();
    let observed = Rc::new(std::cell::Cell::new(false));
    let observed_callback = Rc::clone(&observed);
    let result = fs::SourceDirectory::observe_control_replies(
        move || {
            if ledger(&observed_root)["pending"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["native_birth"]["state"] == "Reserved")
            {
                observed_callback.set(true);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "lost reservation reply",
                ));
            }
            Ok(())
        },
        || {
            db.create_search_projection_consumer(
                id("reservation-reply"),
                &path,
                options(100),
                initialize,
            )
        },
    )
    .unwrap();
    assert!(observed.get());
    assert!(matches!(
        result,
        Err(Error::Database(HawDBError::Storage(_)))
    ));
    assert!(!path.exists());
    let before = ledger(&root);
    append(&mut db, 2);
    let consumer = db
        .create_search_projection_consumer(id("reservation-reply"), &path, options(100), initialize)
        .unwrap();
    let after = ledger(&root);
    assert_eq!(before["high_water"], after["high_water"]);
    assert_eq!(
        before["pending"][0]["registration_uuid"],
        consumer
            .projection()
            .receipt()
            .binding
            .registration_uuid
            .to_string()
    );
    assert_eq!(
        before["pending"][0]["checkpoint_uuid"],
        after["pending"][0]["checkpoint_uuid"]
    );
    assert_eq!(consumer.search_index().unwrap().document_count(), 2);
}

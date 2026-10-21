// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use hawdb_qos::{
    IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimePermit, RuntimeWorkRequest,
};
use hawdb_search::consumer::{ConsumerPreparedCandidate, ConsumerReadSlot, ConsumerReopenLimits};
use hawdb_storage::consumer_allocation::{ConsumerReopenRequest, ConsumerSelectionReceipt};
use hawdb_storage::file_io::SourceDirectory;
use std::sync::Arc;

fn governor() -> RuntimeGovernor {
    RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(512 << 20),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    )
}

fn admit(governor: &RuntimeGovernor, bytes: u64) -> Arc<RuntimePermit> {
    Arc::new(
        governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(bytes)
                    .with_result_bytes(1 << 20)
                    .with_io_wave_slots(1),
            )
            .unwrap(),
    )
}

fn limits() -> ConsumerReopenLimits {
    ConsumerReopenLimits {
        metadata: candidate_metadata_limits(),
        artifact_read_bytes: NonZeroU64::new(32 << 20).unwrap(),
        generation_bytes: NonZeroU64::new(64 << 20).unwrap(),
    }
}

fn request(db: &Database) -> ConsumerReopenRequest {
    db.runtime
        .get()
        .unwrap()
        .store
        .prepare_search_consumer_reopen("stationary")
        .unwrap()
}

fn parent(db: &Database, fixture: &Fixture) -> SourceDirectory {
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    SourceDirectory::capture(&fixture.0, &context).unwrap()
}

fn prepared(
    db: &mut Database,
    fixture: &Fixture,
    governor: &RuntimeGovernor,
    content: &str,
) -> (ConsumerPreparedCandidate, crate::PreparedReadDependencies) {
    prepared_document(
        db,
        fixture,
        governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: content.into(),
            embedding: None,
            metadata: Default::default(),
        },
    )
}

fn prepared_document(
    db: &mut Database,
    fixture: &Fixture,
    governor: &RuntimeGovernor,
    document: hawdb_search::SearchDocument,
) -> (ConsumerPreparedCandidate, crate::PreparedReadDependencies) {
    let allocation = reserve_native_generation(db, fixture);
    let mut read = db
        .begin_prepared_read(crate::PreparedReadLimits {
            dependency_entries: std::num::NonZeroUsize::new(8).unwrap(),
            dependency_bytes: std::num::NonZeroUsize::new(4096).unwrap(),
            statement_rows: std::num::NonZeroUsize::new(8).unwrap(),
            statement_payload_bytes: std::num::NonZeroUsize::new(4096).unwrap(),
        })
        .unwrap();
    read.query_with_params("MATCH (n:Memory) RETURN n.content", &Default::default())
        .unwrap();
    let source = read.finish().unwrap();
    let mut birth = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    birth.create().unwrap();
    let mut lease = sealed_native_candidate_document_lease(
        &mut birth,
        fixture,
        source.source_epoch(),
        governor,
        document,
    );
    lease
        .prepare_candidate_view(NonZeroU64::new(32 << 20).unwrap())
        .unwrap();
    let detached = lease.detach_candidate().unwrap();
    let mut candidate = detached.attach_birth(birth).unwrap();
    candidate.record_durable_seal().unwrap();
    db.validate_prepared_consumer_candidate_source(
        &source,
        &candidate,
        crate::ProjectionSourceDurability::Synchronize,
    )
    .unwrap();
    candidate.record_publication_intent().unwrap();
    (candidate, source)
}

fn selected(
    db: &mut Database,
    fixture: &Fixture,
    governor: &RuntimeGovernor,
) -> ConsumerSelectionReceipt {
    let (candidate, source) = prepared(db, fixture, governor, "amber native reopen");
    complete(db, governor, candidate, source)
}

fn complete(
    db: &mut Database,
    governor: &RuntimeGovernor,
    mut candidate: ConsumerPreparedCandidate,
    source: crate::PreparedReadDependencies,
) -> ConsumerSelectionReceipt {
    let permit = admit(governor, 16 << 20);
    candidate
        .prepare_selection(&permit, Default::default())
        .unwrap();
    let slot = candidate.create_read_slot(&permit).unwrap();
    candidate.bind_read_slot(&slot, &permit).unwrap();
    let mut attempt = candidate
        .admit_selection(Arc::clone(&permit), Default::default())
        .unwrap();
    let receipt = db
        .select_prepared_consumer_candidate(&source, &mut attempt)
        .unwrap()
        .finish()
        .unwrap();
    drop(attempt);
    drop(candidate);
    drop(slot);
    drop(permit);
    receipt
}

fn reopen(
    db: &Database,
    parent: &SourceDirectory,
    governor: &RuntimeGovernor,
    options: ConsumerReopenLimits,
) -> Result<ConsumerReadSlot> {
    ConsumerReadSlot::reopen(
        request(db),
        parent.duplicate(&parent.operation_context()).unwrap(),
        admit(governor, 64 << 20),
        Default::default(),
        options,
    )
}

fn verify_query(slot: &ConsumerReadSlot, governor: &RuntimeGovernor) {
    #[cfg(feature = "full-text-search")]
    {
        let permit = admit(governor, 16 << 20);
        let pin = slot.pin(&permit).unwrap();
        let output = pin
            .probe_query(
                &permit,
                Default::default(),
                "amber",
                None,
                hawdb_search::SearchMode::Text,
                hawdb_search::SearchQueryOptions {
                    limit: 1,
                    offset: 0,
                    rank_window: None,
                    fusion_weights: Default::default(),
                    metadata_filters: Default::default(),
                    policy_epoch: None,
                },
            )
            .unwrap();
        assert_eq!(output.result().total_hits, 1);
        assert_eq!(output.result().hits[0].id, "memory:m1");
    }
    #[cfg(not(feature = "full-text-search"))]
    {
        let _ = governor;
        assert_eq!(slot.report().unwrap().document_count, 1);
    }
}

#[test]
fn consumer_native_reopen_complete_selection_survives_all_old_handles_and_source_close() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    let receipt = selected(&mut db, &fixture, &governor);
    let before = allocation_evidence(&fixture).0;
    drop(db);
    let db = fixture.database();
    let parent = parent(&db, &fixture);
    let slot = reopen(&db, &parent, &governor, limits()).unwrap();
    assert_eq!(slot.report().unwrap().source_epoch, receipt.source_epoch());
    assert_eq!(
        slot.report().unwrap().metadata_reference,
        receipt.metadata_reference()
    );
    assert_eq!(allocation_evidence(&fixture).0, before);
    verify_query(&slot, &governor);
    drop(db);
    // The returned reader must retain no canonical source write lease.
    let reopened_db = fixture.database();
    verify_query(&slot, &governor);
    drop(reopened_db);
    drop(slot);
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
}

#[cfg(all(feature = "full-text-search", feature = "vector-search"))]
#[test]
fn consumer_native_reopen_queries_real_vector_hybrid_and_filtered_text_with_one_io_slot() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    let (candidate, source) = prepared_document(
        &mut db,
        &fixture,
        &governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: "amber native reopen".into(),
            embedding: Some(vec![1.0, 0.0]),
            metadata: std::collections::BTreeMap::from([("space_id".into(), "team".into())]),
        },
    );
    complete(&mut db, &governor, candidate, source);
    drop(db);
    let db = fixture.database();
    let parent = parent(&db, &fixture);
    // Opening a reader needs working/retained memory, not a query result grant.
    let permit = Arc::new(
        governor
            .try_admit(RuntimeWorkRequest::background_maintenance(64 << 20).with_io_wave_slots(1))
            .unwrap(),
    );
    let slot = ConsumerReadSlot::reopen(request(&db), parent, permit, Default::default(), limits())
        .unwrap();
    for mode in [
        hawdb_search::SearchMode::Text,
        hawdb_search::SearchMode::Vector,
        hawdb_search::SearchMode::Hybrid,
    ] {
        let permit = admit(&governor, 16 << 20);
        let pin = slot.pin(&permit).unwrap();
        let result = pin
            .probe_query(
                &permit,
                Default::default(),
                "amber",
                Some(&[1.0, 0.0]),
                mode,
                hawdb_search::SearchQueryOptions {
                    limit: 1,
                    offset: 0,
                    rank_window: None,
                    fusion_weights: Default::default(),
                    policy_epoch: None,
                    metadata_filters: std::collections::BTreeMap::from([(
                        "space_id".into(),
                        "team".into(),
                    )]),
                },
            )
            .unwrap();
        assert_eq!(result.result().total_hits, 1);
        assert_eq!(result.result().hits[0].id, "memory:m1");
    }
    drop(slot);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[cfg(unix)]
#[test]
fn consumer_native_reopen_uses_original_parent_after_namespace_replacement() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    selected(&mut db, &fixture, &governor);
    let parent = parent(&db, &fixture);
    let request = request(&db);
    let original = Fixture(fixture.0.with_extension("original"));
    std::fs::rename(&fixture.0, &original.0).unwrap();
    std::fs::create_dir(&fixture.0).unwrap();
    std::fs::write(fixture.0.join("foreign"), b"untouched").unwrap();
    let slot = ConsumerReadSlot::reopen(
        request,
        parent,
        admit(&governor, 64 << 20),
        Default::default(),
        limits(),
    )
    .unwrap();
    verify_query(&slot, &governor);
    assert_eq!(
        std::fs::read(fixture.0.join("foreign")).unwrap(),
        b"untouched"
    );
    assert_eq!(std::fs::read_dir(&fixture.0).unwrap().count(), 1);
    drop(slot);
    drop(db);
}

#[test]
fn consumer_native_reopen_refuses_working_fd_and_verification_budgets_then_retries() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    selected(&mut db, &fixture, &governor);
    let parent = parent(&db, &fixture);
    let before = allocation_evidence(&fixture).0;
    let descriptors = hawdb_storage::file_descriptors::ProjectFileDescriptors::from_open_context(
        &parent.operation_context(),
    );
    let baseline = descriptors.metrics().open;
    for bytes in [8 << 20, 16 << 20] {
        let small = ConsumerReadSlot::reopen(
            request(&db),
            parent.duplicate(&parent.operation_context()).unwrap(),
            admit(&governor, bytes),
            Default::default(),
            limits(),
        );
        assert!(matches!(small, Err(HawDBError::ResourceBudgetExceeded(_))));
        assert_eq!(descriptors.metrics().open, baseline);
    }
    let cancellation = hawdb_core::RuntimeCancellationToken::new();
    cancellation.cancel();
    let cancelled = ConsumerReadSlot::reopen(
        request(&db),
        parent.duplicate(&parent.operation_context()).unwrap(),
        admit(&governor, 64 << 20),
        hawdb_core::RuntimeTaskContext::without_deadline(cancellation),
        limits(),
    );
    assert!(matches!(cancelled, Err(HawDBError::Execution(_))));
    assert_eq!(descriptors.metrics().open, baseline);
    let foreign = Fixture::new();
    let foreign_db = foreign.database();
    let foreign_parent = self::parent(&foreign_db, &foreign);
    let error = ConsumerReadSlot::reopen(
        request(&db),
        foreign_parent,
        admit(&governor, 64 << 20),
        Default::default(),
        limits(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("descriptor domain"), "{error}");
    assert_eq!(descriptors.metrics().open, baseline);
    let held = descriptors.reserve(1).unwrap();
    let refused = reopen(&db, &parent, &governor, limits()).unwrap_err();
    assert!(
        hawdb_core::error::file_descriptor_error(&refused).is_some(),
        "{refused}"
    );
    drop(held);
    assert_eq!(descriptors.metrics().open, baseline);
    for failure in 0..3 {
        let mut options = limits();
        match failure {
            0 => options.metadata.encoded_bytes = 1,
            1 => options.artifact_read_bytes = NonZeroU64::MIN,
            _ => options.generation_bytes = NonZeroU64::MIN,
        }
        assert!(reopen(&db, &parent, &governor, options).is_err());
        assert_eq!(descriptors.metrics().open, baseline);
    }
    assert_eq!(allocation_evidence(&fixture).0, before);
    let slot = reopen(&db, &parent, &governor, limits()).unwrap();
    verify_query(&slot, &governor);
}

#[test]
fn consumer_native_reopen_keeps_unresolved_intents_busy_without_publishing_receipts() {
    for native_attempt in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let governor = governor();
        let (mut candidate, source) = prepared(&mut db, &fixture, &governor, "amber native reopen");
        let permit = admit(&governor, 16 << 20);
        candidate
            .prepare_selection(&permit, Default::default())
            .unwrap();
        let slot = candidate.create_read_slot(&permit).unwrap();
        candidate.bind_read_slot(&slot, &permit).unwrap();
        let mut attempt = candidate
            .admit_selection(Arc::clone(&permit), Default::default())
            .unwrap();
        if native_attempt {
            drop(
                db.select_prepared_consumer_candidate(&source, &mut attempt)
                    .unwrap(),
            );
        }
        drop(attempt);
        drop(candidate);
        drop(slot);
        drop(permit);
        let before = allocation_evidence(&fixture).0;
        drop(db);
        let db = fixture.database();
        let parent = parent(&db, &fixture);
        assert!(matches!(
            reopen(&db, &parent, &governor, limits()),
            Err(HawDBError::ProjectionBusy)
        ));
        assert_eq!(allocation_evidence(&fixture).0, before);
    }
}

#[test]
fn consumer_native_reopen_preserves_tamper_evidence_and_validates_the_complete_closure() {
    for corruption in 0..6 {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let governor = governor();
        let receipt = selected(&mut db, &fixture, &governor);
        let (original_ledger, evidence) = allocation_evidence(&fixture);
        let target = fixture
            .0
            .join(evidence["selected"][0]["generation_name"].as_str().unwrap());
        let artifact = || {
            std::fs::read_dir(&target)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .and_then(|n| n.strip_suffix(".hawdb"))
                        .and_then(|stem| stem.rsplit('.').next())
                        .is_some_and(|n| n.parse::<u64>().is_ok())
                })
                .unwrap()
        };
        let file = match corruption {
            0 => target.join("consumer-birth.hawdb"),
            1 | 4 | 5 => target.join("consumer-generation.meta.hawdb"),
            2 => target.join("search_projection.out_of_core.manifest.hawdb"),
            _ => artifact(),
        };
        let original = std::fs::read(&file).unwrap();
        let mut changed = original.clone();
        if corruption == 5 {
            std::fs::remove_file(&file).unwrap();
        } else {
            changed[if corruption == 4 { 8 } else { 0 }] ^= 1;
            if corruption == 0 {
                let end = changed.len() - 4;
                let crc = hawdb_integrity::crc32c(&changed[..end]);
                changed[end..].copy_from_slice(&crc.get().to_le_bytes());
            }
            if corruption == 4 {
                let end = changed.len() - 32;
                let mut hasher = hawdb_integrity::IntegrityHasher::new();
                hasher.update(&changed[..end]);
                changed[end..].copy_from_slice(hasher.finish().sha256.as_bytes());
                let replacement = candidate_reference(&changed).sha256.to_string();
                let prior = receipt.metadata_reference().sha256.to_string();
                let body =
                    std::str::from_utf8(&original_ledger[..original_ledger.len() - 4]).unwrap();
                assert_eq!(body.matches(&prior).count(), 2);
                let mut bytes = body.replace(&prior, &replacement).into_bytes();
                let crc = hawdb_integrity::crc32c(&bytes);
                bytes.extend_from_slice(&crc.get().to_le_bytes());
                std::fs::write(fixture.0.join("database/consumer-allocations.hawdb"), bytes)
                    .unwrap();
            }
            std::fs::write(&file, &changed).unwrap();
        }
        let observed_ledger = allocation_evidence(&fixture).0;
        let parent = parent(&db, &fixture);
        let error = reopen(&db, &parent, &governor, limits()).unwrap_err();
        if corruption == 4 {
            assert!(
                error
                    .to_string()
                    .contains("metadata differs from selected receipt"),
                "{error}"
            );
        }
        assert_eq!(allocation_evidence(&fixture).0, observed_ledger);
        if corruption == 5 {
            assert!(!file.exists());
        } else {
            assert_eq!(std::fs::read(&file).unwrap(), changed);
        }
        std::fs::write(&file, original).unwrap();
        std::fs::write(
            fixture.0.join("database/consumer-allocations.hawdb"),
            original_ledger,
        )
        .unwrap();
        let slot = reopen(&db, &parent, &governor, limits()).unwrap();
        verify_query(&slot, &governor);
    }
}

#[test]
fn consumer_native_reopened_slot_accepts_successor_without_replacing_old_pins() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    let first = selected(&mut db, &fixture, &governor);
    let parent = parent(&db, &fixture);
    let slot = reopen(&db, &parent, &governor, limits()).unwrap();
    let pin_permit = admit(&governor, 16 << 20);
    let old = slot.pin(&pin_permit).unwrap();
    let held_permit = admit(&governor, 64 << 20);
    let task = held_permit.bind_task_context(Default::default());
    let io = task.acquire_io_wave(std::num::NonZeroUsize::MIN).unwrap();
    let held = request(&db)
        .open(
            parent.duplicate(&parent.operation_context()).unwrap(),
            &task,
        )
        .unwrap();
    drop(io);
    let (mut candidate, source) = prepared(&mut db, &fixture, &governor, "violet successor");
    let permit = admit(&governor, 16 << 20);
    candidate
        .prepare_selection(&permit, Default::default())
        .unwrap();
    candidate.bind_read_slot(&slot, &permit).unwrap();
    let mut attempt = candidate
        .admit_selection(Arc::clone(&permit), Default::default())
        .unwrap();
    let pending = db
        .select_prepared_consumer_candidate(&source, &mut attempt)
        .unwrap();
    assert!(matches!(
        slot.pin(&pin_permit),
        Err(HawDBError::ProjectionBusy)
    ));
    let second = pending.finish().unwrap();
    drop(attempt);
    assert_eq!(
        slot.report().unwrap().metadata_reference,
        second.metadata_reference()
    );
    assert_eq!(old.report().metadata_reference, first.metadata_reference());
    let io = task.acquire_io_wave(std::num::NonZeroUsize::MIN).unwrap();
    assert!(matches!(
        held.require_current(&task),
        Err(HawDBError::ProjectionBusy)
    ));
    drop(io);
    #[cfg(feature = "full-text-search")]
    {
        let options = || hawdb_search::SearchQueryOptions {
            limit: 1,
            offset: 0,
            rank_window: None,
            fusion_weights: Default::default(),
            metadata_filters: Default::default(),
            policy_epoch: None,
        };
        let prior = old
            .probe_query(
                &pin_permit,
                Default::default(),
                "amber",
                None,
                hawdb_search::SearchMode::Text,
                options(),
            )
            .unwrap();
        assert_eq!(prior.result().total_hits, 1);
        let next = slot
            .pin(&pin_permit)
            .unwrap()
            .probe_query(
                &pin_permit,
                Default::default(),
                "violet",
                None,
                hawdb_search::SearchMode::Text,
                options(),
            )
            .unwrap();
        assert_eq!(next.result().total_hits, 1);
    }
}

#[test]
fn consumer_native_reopen_process_child() {
    let Ok(root) = std::env::var("HAWDB_NATIVE_REOPEN_FIXTURE") else {
        return;
    };
    let root = PathBuf::from(root);
    let db = Database::open(root.join("database")).unwrap();
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let parent = SourceDirectory::capture(&root, &context).unwrap();
    let governor = governor();
    let slot = reopen(&db, &parent, &governor, limits()).unwrap();
    verify_query(&slot, &governor);
    let report = slot.report().unwrap();
    drop(slot);
    drop(db);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    std::fs::write(root.join("reopen-process-evidence.json"), serde_json::to_vec(&serde_json::json!({
        "source_epoch": report.source_epoch, "metadata_sha256": report.metadata_reference.sha256.to_string(),
        "document_count": report.document_count,
    })).unwrap()).unwrap();
}

#[test]
fn consumer_native_reopen_new_process_reconstructs_completed_native_custody() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = governor();
    let receipt = selected(&mut db, &fixture, &governor);
    drop(db);
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "api::search_projection_consumer::tests::native_reopen::consumer_native_reopen_process_child", "--nocapture"])
        .env("HAWDB_NATIVE_REOPEN_FIXTURE", &fixture.0).output().unwrap();
    assert!(
        output.status.success(),
        "child stdout: {}\nchild stderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let evidence: serde_json::Value = serde_json::from_slice(
        &std::fs::read(fixture.0.join("reopen-process-evidence.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(evidence["source_epoch"], receipt.source_epoch());
    assert_eq!(
        evidence["metadata_sha256"],
        receipt.metadata_reference().sha256.to_string()
    );
    assert_eq!(evidence["document_count"], 1);
}

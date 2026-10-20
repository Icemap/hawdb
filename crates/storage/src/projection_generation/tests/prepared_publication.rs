// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        Self(path("prepared-publication"))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn prepare(
    store: &ProjectionGenerationStore,
    sealed: &SealedProjectionGeneration,
) -> Box<PreparedProjectionPublication> {
    match store.prepare_publication(sealed).unwrap() {
        ProjectionGenerationPreparation::Prepared(prepared) => prepared,
        ProjectionGenerationPreparation::AlreadyPublished(_) => {
            panic!("unexpected published generation")
        }
    }
}

#[test]
fn preparation_is_invisible_and_pending_blocks_competitors_until_durable() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let old = stage(&store, begin("g1", None, 1), &members(&[("a", "old")]));
    store.publish(&old).unwrap();
    let old_path = store.data_path(&old.manifest.begin.identity);
    drop(old);
    let sealed = stage(
        &store,
        begin("g2", Some("g1"), 2),
        &members(&[("a", "new")]),
    );
    let mut prepared = prepare(&store, &sealed);
    assert_eq!(
        store
            .open_active("session_spans", b"session-1")
            .unwrap()
            .manifest()
            .begin
            .identity
            .generation,
        "g1"
    );
    assert!(matches!(
        store.prepare_publication(&sealed),
        Err(ProjectionGenerationError::Busy)
    ));
    let pending = prepared.select().unwrap();
    assert!(matches!(
        store.published_receipt(&sealed),
        Err(ProjectionGenerationError::Busy)
    ));
    assert_eq!(
        store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap()
            .generations_reclaimed,
        0
    );
    assert!(store.file_exists(&old_path).unwrap());
    // A different owner can progress while this operation retains its custody.
    let other = stage(&store, begin_for(b"other", "g1", None, 1), &[]);
    store.publish(&other).unwrap();
    let report = pending.finish().unwrap();
    assert!(!report.idempotent);
    assert_eq!(report.previous_generation.as_deref(), Some("g1"));
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    assert_eq!(
        store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap()
            .generations_reclaimed,
        1
    );
    assert!(!store.file_exists(&old_path).unwrap());
}

#[test]
fn dropped_pending_requires_original_owner_resolution_before_new_publication() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let sealed = stage(&store, begin("g1", None, 1), &[]);
    let mut prepared = prepare(&store, &sealed);
    drop(prepared.select().unwrap());
    assert!(matches!(
        store.published_receipt(&sealed),
        Err(ProjectionGenerationError::Busy)
    ));
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::PublicationUncertain)
    ));
    assert!(matches!(
        prepared.resolve().unwrap(),
        ProjectionGenerationResolution::Published(_)
    ));
    assert!(store.published_receipt(&sealed).unwrap().is_some());
}

#[test]
fn dropped_attempted_owner_retains_gc_roots_and_can_be_resolved_by_matching_token() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let old = stage(&store, begin("g1", None, 1), &[]);
    store.publish(&old).unwrap();
    let old_path = store.data_path(&old.manifest.begin.identity);
    drop(old);
    let sealed = stage(&store, begin("g2", Some("g1"), 2), &[]);
    let mut prepared = prepare(&store, &sealed);
    drop(prepared.select().unwrap());
    drop(prepared);
    assert!(matches!(
        store.prepare_publication(&sealed),
        Err(ProjectionGenerationError::PublicationUncertain)
    ));
    assert_eq!(
        store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap()
            .generations_reclaimed,
        0
    );
    assert!(store.file_exists(&old_path).unwrap());
    let unrelated = stage(&store, begin("g3", Some("g2"), 3), &[]);
    assert!(matches!(
        store.published_receipt(&unrelated),
        Err(ProjectionGenerationError::PublicationUncertain)
    ));
    assert!(store.published_receipt(&sealed).unwrap().is_some());
    store.publish(&unrelated).unwrap();
}

#[test]
fn mismatched_selected_manifest_reports_conflict_and_retains_recovery_roots() {
    for retain_owner in [true, false] {
        let fixture = Fixture::new();
        let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
        let old = stage(&store, begin("g1", None, 1), &[]);
        store.publish(&old).unwrap();
        let old_identity = old.manifest.begin.identity.clone();
        drop(old);
        let sealed = stage(&store, begin("g2", Some("g1"), 2), &[]);
        let new_identity = sealed.manifest.begin.identity.clone();
        let mut prepared = prepare(&store, &sealed);
        drop(prepared.select().unwrap());
        let mut prepared = if retain_owner {
            Some(prepared)
        } else {
            drop(prepared);
            None
        };

        // Model external content reuse after selection, without declaring
        // either the original operation or its durability resolved.
        let path = store.head_path("session_spans", b"session-1");
        let mut head = store.read_optional_head(&path).unwrap().unwrap();
        head.manifest.begin.source_watermark += 1;
        let bytes = encode_head(&head).unwrap();
        store
            .write_control(&path, &bytes, &mut store.lock_shared().unwrap())
            .unwrap();
        for _ in 0..2 {
            let result = if let Some(prepared) = &mut prepared {
                prepared.resolve().map(|_| ())
            } else {
                store.published_receipt(&sealed).map(|_| ())
            };
            assert!(
                matches!(result, Err(ProjectionGenerationError::Conflict(message))
                if message == "published generation identity was reused with different content")
            );
            assert_eq!(store.read_optional_head(&path).unwrap(), Some(head.clone()));
        }
        assert!(matches!(
            store.open_active("session_spans", b"session-1"),
            Err(ProjectionGenerationError::Corruption(_))
        ));
        drop(prepared);
        assert!(matches!(
            store.prepare_publication(&sealed),
            Err(ProjectionGenerationError::PublicationUncertain)
        ));
        assert!(matches!(
            store.publish(&sealed),
            Err(ProjectionGenerationError::Conflict(_))
        ));
        drop(sealed);
        let status = store.status(&new_identity).unwrap();
        assert!(!status.pinned);
        assert_eq!(
            status.state,
            ProjectionGenerationState::PublicationUncertain
        );
        let reclaimed = store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap();
        assert_eq!(reclaimed.generations_reclaimed, 0);
        assert_eq!(reclaimed.pinned_generations_skipped, 2);
        for identity in [old_identity, new_identity] {
            assert!(store.file_exists(&store.data_path(&identity)).unwrap());
            assert!(store.file_exists(&store.manifest_path(&identity)).unwrap());
        }
        let other = stage(&store, begin_for(b"other", "g1", None, 1), &[]);
        store.publish(&other).unwrap();
    }
}

#[test]
fn missing_private_candidate_resolves_not_selected_without_reusing_attempt() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let sealed = stage(&store, begin("g1", None, 1), &[]);
    let mut prepared = prepare(&store, &sealed);
    let names = std::fs::read_dir(fixture.0.join("heads"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(names.len(), 1);
    // External namespace disruption before the native effect. The owned file
    // remains held and attempted; a missing response does not imply rollback.
    std::fs::remove_file(names[0].path()).unwrap();
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Io(_))
    ));
    assert_eq!(
        prepared.resolve().unwrap(),
        ProjectionGenerationResolution::NotSelected
    );
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    store.publish(&sealed).unwrap();
}

#[test]
fn try_lock_busy_does_not_consume_prepared_selection() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let sealed = stage(&store, begin("g1", None, 1), &[]);
    let mut prepared = prepare(&store, &sealed);
    let guard = store.lock_shared().unwrap();
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Busy)
    ));
    drop(guard);
    prepared.select().unwrap().finish().unwrap();
}

#[test]
fn stale_head_identity_rejects_equal_bytes_without_overwriting_replacement() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let old = stage(&store, begin("g1", None, 1), &[]);
    store.publish(&old).unwrap();
    let sealed = stage(&store, begin("g2", Some("g1"), 2), &[]);
    let mut prepared = prepare(&store, &sealed);
    let head = store.head_path("session_spans", b"session-1");
    let bytes = std::fs::read(&head).unwrap();
    let replacement = head.with_extension("replacement");
    std::fs::write(&replacement, &bytes).unwrap();
    std::fs::rename(&replacement, &head).unwrap();
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    assert_eq!(std::fs::read(&head).unwrap(), bytes);
    assert!(matches!(
        prepared.select(),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    drop(prepared);
    store.publish(&sealed).unwrap();
}

#[test]
fn reservation_exhaustion_fails_before_creating_control_candidate() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let sealed = stage(&store, begin("g1", None, 1), &[]);
    store.lock_shared().unwrap().publication_sequence = u64::MAX;
    assert!(matches!(
        store.prepare_publication(&sealed),
        Err(ProjectionGenerationError::Admission(_))
    ));
    assert_eq!(
        std::fs::read_dir(fixture.0.join("heads")).unwrap().count(),
        0
    );
}

#[test]
fn publication_epoch_exhaustion_keeps_old_head_and_creates_no_selector() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(&fixture.0).unwrap();
    let old = stage(&store, begin("g1", None, 1), &[]);
    store.publish(&old).unwrap();
    let path = store.head_path("session_spans", b"session-1");
    let mut head = store.read_optional_head(&path).unwrap().unwrap();
    head.publication_commit_epoch = u64::MAX;
    store
        .write_control(
            &path,
            &encode_head(&head).unwrap(),
            &mut store.lock_shared().unwrap(),
        )
        .unwrap();
    let before = std::fs::read_dir(fixture.0.join("heads")).unwrap().count();
    let next = stage(&store, begin("g2", Some("g1"), 2), &[]);
    assert!(matches!(
        store.prepare_publication(&next),
        Err(ProjectionGenerationError::Admission(_))
    ));
    assert_eq!(store.read_optional_head(&path).unwrap().unwrap(), head);
    assert_eq!(
        std::fs::read_dir(fixture.0.join("heads")).unwrap().count(),
        before
    );
}

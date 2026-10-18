// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::file_descriptors::ProjectFileDescriptors;
use crate::power_loss::{image::ImageLimits, PowerLossModel};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = path("captured-catalog");
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for directory in ["heads", "generations"] {
        for entry in std::fs::read_dir(root.join(directory)).unwrap() {
            let entry = entry.unwrap();
            files.insert(
                Path::new(directory).join(entry.file_name()),
                std::fs::read(entry.path()).unwrap(),
            );
        }
    }
    files
}

#[test]
fn publication_pages_and_gc_use_one_captured_catalog_after_path_reuse() {
    for recorded in [false, true] {
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, 32).unwrap();
        let model =
            recorded.then(|| PowerLossModel::attach(&project, ImageLimits::default()).unwrap());
        let named = fixture.0.join("named");
        let moved = fixture.0.join("moved");
        let store = ProjectionGenerationStore::open(&named).unwrap();
        let old_rows = members(&[("a", "original source")]);
        let initial = stage(&store, begin("g1", None, 1), &old_rows);
        store.publish(&initial).unwrap();
        drop(initial);
        let reader = store.open_active("session_spans", b"session-1").unwrap();
        let readonly = ProjectionGenerationStore::open_existing(&named).unwrap();
        let obsolete = stage(&store, begin("obsolete", Some("g1"), 2), &old_rows);
        let obsolete_data = store.data_path(&obsolete.manifest.begin.identity);
        drop(obsolete);
        // Keep an actual writer across the namespace transition and B's writer.
        let mut writer = store
            .begin_candidate(
                begin("g2", Some("g1"), 2),
                ProjectionGenerationBatchLimits::default(),
            )
            .unwrap();
        fs::rename(&named, &moved).unwrap();

        let foreign = ProjectionGenerationStore::open(&named).unwrap();
        assert!(!Arc::ptr_eq(&store.shared, &foreign.shared));
        let foreign_rows = members(&[("a", "foreign source")]);
        let foreign_head = stage(&foreign, begin("g1", None, 1), &foreign_rows);
        foreign.publish(&foreign_head).unwrap();
        let foreign_pending = stage(&foreign, begin("g2", Some("g1"), 2), &foreign_rows);
        let foreign_obsolete = stage(&foreign, begin("obsolete", Some("g1"), 2), &foreign_rows);
        let foreign_before = tree(&named);

        let next_rows = members(&[("a", "new original source")]);
        writer.append_batch(&next_rows).unwrap();
        let next = writer.seal(seal_for(&next_rows)).unwrap();
        store.publish(&next).unwrap();
        assert_eq!(
            store.status(&next.manifest.begin.identity).unwrap().state,
            ProjectionGenerationState::Published
        );
        assert_eq!(
            reader
                .read_page(None, ProjectionGenerationReadLimits::default())
                .unwrap()
                .members,
            old_rows
        );
        reader.scrub().unwrap();
        let active = readonly.open_active("session_spans", b"session-1").unwrap();
        assert_eq!(
            active
                .read_page(None, ProjectionGenerationReadLimits::default())
                .unwrap()
                .members,
            next_rows
        );
        active.scrub().unwrap();
        let reclaimed = store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap();
        assert_eq!(reclaimed.generations_reclaimed, 1);
        assert!(!moved
            .join("generations")
            .join(obsolete_data.file_name().unwrap())
            .exists());
        assert!(moved
            .join("generations")
            .join(
                store
                    .data_path(&next.manifest.begin.identity)
                    .file_name()
                    .unwrap()
            )
            .exists());
        assert_eq!(tree(&named), foreign_before);
        assert_eq!(
            store.published_receipt(&next).unwrap().unwrap().identity,
            next.manifest.begin.identity
        );
        assert!(matches!(
            foreign.publish(&next),
            Err(ProjectionGenerationError::Conflict(_))
        ));
        drop((
            active,
            reader,
            next,
            readonly,
            store,
            foreign_obsolete,
            foreign_pending,
            foreign_head,
            foreign,
        ));
        drop(model);
        assert_eq!(project.metrics().open, 0);
    }
}

#[test]
fn torn_tail_repair_resumes_the_original_candidate_not_the_reused_path() {
    let fixture = Fixture::new();
    let named = fixture.0.join("named");
    let moved = fixture.0.join("moved");
    let store = ProjectionGenerationStore::open(&named).unwrap();
    let rows = members(&[("a", "first"), ("b", "second")]);
    let mut writer = store
        .begin_candidate(
            begin("g1", None, 1),
            ProjectionGenerationBatchLimits::default(),
        )
        .unwrap();
    writer.append_batch(&rows[..1]).unwrap();
    let data_path = store.data_path(&writer.begin.identity);
    drop(writer);
    {
        let mut file = OpenOptions::new().append(true).open(&data_path).unwrap();
        file.write_all(&[4, 0]).unwrap();
        file.sync_all().unwrap();
    }
    fs::rename(&named, &moved).unwrap();
    let foreign = ProjectionGenerationStore::open(&named).unwrap();
    let foreign_rows = members(&[("a", "foreign complete data")]);
    let foreign_head = stage(&foreign, begin("g1", None, 1), &foreign_rows);
    foreign.publish(&foreign_head).unwrap();
    let foreign_before = tree(&named);
    let mut resumed = store
        .begin_candidate(
            begin("g1", None, 1),
            ProjectionGenerationBatchLimits::default(),
        )
        .unwrap();
    assert!(resumed.recovered_torn_tail());
    resumed.append_batch(&rows[1..]).unwrap();
    let sealed = resumed.seal(seal_for(&rows)).unwrap();
    store.publish(&sealed).unwrap();
    let reader = store.open_active("session_spans", b"session-1").unwrap();
    reader.scrub().unwrap();
    assert_eq!(
        reader
            .read_page(None, ProjectionGenerationReadLimits::default())
            .unwrap()
            .members,
        rows
    );
    assert_eq!(tree(&named), foreign_before);
}

#[test]
fn identical_catalog_copy_cannot_adopt_an_original_sealed_token() {
    let fixture = Fixture::new();
    let named = fixture.0.join("named");
    let moved = fixture.0.join("moved");
    let store = ProjectionGenerationStore::open(&named).unwrap();
    let rows = members(&[("a", "identical bytes")]);
    let original = stage(&store, begin("g1", None, 1), &rows);
    let bytes = tree(&named);
    fs::rename(&named, &moved).unwrap();
    fs::create_dir(&named).unwrap();
    for directory in ["generations", "heads"] {
        fs::create_dir(named.join(directory)).unwrap();
    }
    for (name, bytes) in &bytes {
        fs::write(named.join(name), bytes).unwrap();
    }
    let foreign = ProjectionGenerationStore::open(&named).unwrap();
    assert!(!Arc::ptr_eq(&store.shared, &foreign.shared));
    assert!(matches!(
        foreign.publish(&original),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    assert_eq!(tree(&named), bytes);
    store.publish(&original).unwrap();
}

#[test]
fn capture_and_candidate_admission_use_the_existing_descriptor_domain() {
    for limit in [2, 3] {
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, limit).unwrap();
        let opened = ProjectionGenerationStore::open(fixture.0.join("catalog"));
        if limit == 2 {
            assert!(matches!(opened, Err(ProjectionGenerationError::Io(_))));
        } else {
            let store = opened.unwrap();
            assert_eq!(project.metrics().open, 3);
            assert!(matches!(
                store.begin_candidate(
                    begin("g1", None, 1),
                    ProjectionGenerationBatchLimits::default()
                ),
                Err(ProjectionGenerationError::Io(_))
            ));
            assert_eq!(
                std::fs::read_dir(fixture.0.join("catalog/generations"))
                    .unwrap()
                    .count(),
                0
            );
            drop(store);
        }
        assert_eq!(project.metrics().open, 0);
    }
}

#[test]
fn selecting_a_head_and_registering_its_reader_pin_have_no_gc_gap() {
    let fixture = Fixture::new();
    let store = ProjectionGenerationStore::open(fixture.0.join("catalog")).unwrap();
    let rows = members(&[("a", "old selected bytes")]);
    let initial = stage(&store, begin("g1", None, 1), &rows);
    store.publish(&initial).unwrap();
    let old_data = store.data_path(&initial.manifest.begin.identity);
    drop(initial);
    let (captured_tx, captured_rx) = std::sync::mpsc::sync_channel(0);
    let (release_tx, release_rx) = std::sync::mpsc::sync_channel(0);
    let (reader, excluded) = std::thread::scope(|scope| {
        let store = &store;
        let worker = scope.spawn(move || {
            AFTER_ACTIVE_HEAD.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move || {
                    captured_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }));
            });
            store.open_active("session_spans", b"session-1").unwrap()
        });
        captured_rx.recv().unwrap();
        // Observe the actual mutex while native head lookup has finished but
        // the pin is not registered. No sleeps or scheduler deadline inference.
        let excluded = matches!(
            store.shared.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        );
        release_tx.send(()).unwrap();
        (worker.join().unwrap(), excluded)
    });
    assert!(
        excluded,
        "publication/GC entered between head selection and pin registration"
    );
    let next = stage(
        &store,
        begin("g2", Some("g1"), 2),
        &members(&[("a", "new selected bytes")]),
    );
    store.publish(&next).unwrap();
    assert_eq!(
        store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap()
            .generations_reclaimed,
        0
    );
    assert!(old_data.exists());
    assert_eq!(
        reader
            .read_page(None, ProjectionGenerationReadLimits::default())
            .unwrap()
            .members,
        rows
    );
    drop(reader);
    assert_eq!(
        store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap()
            .generations_reclaimed,
        1
    );
    assert!(!old_data.exists());
}

#[test]
fn lost_selection_requires_a_head_barrier_before_receipt_retry_or_gc_retirement() {
    for resolution in ["receipt", "idempotent", "gc"] {
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, 32).unwrap();
        let model = PowerLossModel::attach(&project, ImageLimits::default()).unwrap();
        let store = ProjectionGenerationStore::open(fixture.0.join("catalog")).unwrap();
        let old_rows = members(&[("a", "old durable generation")]);
        let initial = stage(&store, begin("g1", None, 1), &old_rows);
        store.publish(&initial).unwrap();
        drop(initial);
        let rows = members(&[("a", "new durable generation")]);
        let next = stage(&store, begin("g2", Some("g1"), 2), &rows);
        FAIL_SELECTION_BARRIER.with(|failure| failure.set(true));
        assert!(matches!(
            store.publish(&next),
            Err(ProjectionGenerationError::Io(_))
        ));
        let cut = model.capture().unwrap();
        let before = cut.crash(&Default::default()).unwrap();
        recover_catalog_image(&before, "g1", &old_rows);
        match resolution {
            "receipt" => {
                assert!(store.published_receipt(&next).unwrap().is_some());
            }
            "idempotent" => {
                assert!(store.publish(&next).unwrap().idempotent);
            }
            "gc" => {
                assert_eq!(
                    store
                        .reclaim(ProjectionGenerationGcLimits::default())
                        .unwrap()
                        .generations_reclaimed,
                    1
                );
            }
            _ => unreachable!(),
        }
        // Lose every still-unsynchronized namespace/data write. The selected
        // generation must be complete even if old roots were actually removed.
        let after = model.capture().unwrap().crash(&Default::default()).unwrap();
        recover_catalog_image(&after, "g2", &rows);
    }
}

fn recover_catalog_image(
    image: &crate::power_loss::image::CrashImage,
    expected_generation: &str,
    rows: &[ProjectionGenerationMember],
) {
    let fixture = Fixture::new();
    let replay = fixture.0.join("replay");
    image.materialize(&replay).unwrap();
    let store = ProjectionGenerationStore::open_existing(replay.join("catalog")).unwrap();
    let reader = store.open_active("session_spans", b"session-1").unwrap();
    reader.scrub().unwrap();
    assert_eq!(
        reader.manifest().begin.identity.generation,
        expected_generation
    );
    assert_eq!(
        reader
            .read_page(None, ProjectionGenerationReadLimits::default())
            .unwrap()
            .members,
        rows
    );
}

#[test]
fn missing_active_manifest_refuses_gc_without_erasing_its_data() {
    let fixture = Fixture::new();
    let root = fixture.0.join("catalog");
    let store = ProjectionGenerationStore::open(&root).unwrap();
    let initial = stage(
        &store,
        begin("g1", None, 1),
        &members(&[("a", "retained evidence")]),
    );
    store.publish(&initial).unwrap();
    let identity = initial.manifest.begin.identity.clone();
    drop(initial);
    fs::remove_file(store.manifest_path(&identity)).unwrap();
    let before = tree(&root);
    assert!(matches!(
        store.reclaim(ProjectionGenerationGcLimits::default()),
        Err(ProjectionGenerationError::Corruption(_))
    ));
    assert_eq!(tree(&root), before);
}

#[test]
fn final_registry_authority_retires_before_captured_identities_close() {
    let fixture = Fixture::new();
    let project = ProjectFileDescriptors::acquire(&fixture.0, 16).unwrap();
    let mut store = ProjectionGenerationStore::open(fixture.0.join("catalog")).unwrap();
    let observed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let probe = captured::CaptureCloseProbe {
        shared: Arc::downgrade(&store.shared),
        project: project.clone(),
        observed: observed.clone(),
    };
    Arc::get_mut(&mut store.captured).unwrap().before_close = Some(probe);
    drop(store);
    assert!(observed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(project.metrics().open, 0);
}

#[test]
fn an_unacknowledged_append_must_recover_before_more_writes_or_sealing() {
    let next = members(&[("b", "unacknowledged row")]);
    let mut encoded = Vec::new();
    append_record(&encode_member(&next[0]).unwrap(), &mut encoded).unwrap();
    for prefix in [2, encoded.len()] {
        let fixture = Fixture::new();
        let store = ProjectionGenerationStore::open(fixture.0.join("catalog")).unwrap();
        let initial = members(&[("a", "initial row")]);
        let mut writer = store
            .begin_candidate(
                begin("g1", None, 1),
                ProjectionGenerationBatchLimits::default(),
            )
            .unwrap();
        writer.append_batch(&initial).unwrap();
        let path = store.data_path(&writer.begin.identity);
        {
            // Model a native append effect whose error/lost response occurred
            // before the writer installed its new counters/digest/boundary.
            let mut file = store.open_mutable_file(&path, false).unwrap();
            file.seek(SeekFrom::End(0)).unwrap();
            file.write_all(&encoded[..prefix]).unwrap();
            file.sync_all().unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        assert!(matches!(
            writer.append_batch(&members(&[("c", "must not append")])),
            Err(ProjectionGenerationError::Conflict(_))
        ));
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(matches!(
            writer.seal(seal_for(&initial)),
            Err(ProjectionGenerationError::Corruption(_))
        ));
        let mut recovered = store
            .begin_candidate(
                begin("g1", None, 1),
                ProjectionGenerationBatchLimits::default(),
            )
            .unwrap();
        assert_eq!(recovered.recovered_torn_tail(), prefix < encoded.len());
        if prefix < encoded.len() {
            recovered.append_batch(&next).unwrap();
        }
        let last = members(&[("c", "post-recovery row")]);
        recovered.append_batch(&last).unwrap();
        let rows = [initial, next.clone(), last].concat();
        let sealed = recovered.seal(seal_for(&rows)).unwrap();
        store.publish(&sealed).unwrap();
        let reader = store.open_active("session_spans", b"session-1").unwrap();
        reader.scrub().unwrap();
        assert_eq!(
            reader
                .read_page(None, ProjectionGenerationReadLimits::default())
                .unwrap()
                .members,
            rows
        );
    }
}

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Windows refuses ancestor moves with the retained child directories open.
//! Exercise the legal constructor boundary without closing the chosen root.

use super::*;
use crate::file_io::SourceDirectory;

fn finish_capture(label: PathBuf, root: SourceDirectory) -> ProjectionGenerationStore {
    // These are the same constructor steps used by open/open_existing, after
    // their native root capture. The root remains continuously held throughout
    // replacement of its original label; no authority is released/reacquired.
    ProjectionGenerationStore::from_capture(label, CapturedCatalog::from_root(root).unwrap(), false)
        .unwrap()
}

#[test]
fn replacing_the_label_after_root_capture_keeps_all_catalog_phases_on_a() {
    for recorded in [false, true] {
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, 32).unwrap();
        let model =
            recorded.then(|| PowerLossModel::attach(&project, ImageLimits::default()).unwrap());
        let named = fixture.0.join("named");
        let moved = fixture.0.join("moved");
        let old_rows = members(&[("a", "original source")]);
        let label = {
            // Prepare persistent input, then start a fresh capture. This is
            // fixture seeding, not disposal of a live publication owner to
            // force an otherwise forbidden move.
            let seed = ProjectionGenerationStore::open(&named).unwrap();
            let initial = stage(&seed, begin("g1", None, 1), &old_rows);
            seed.publish(&initial).unwrap();
            let obsolete = stage(&seed, begin("obsolete", Some("g1"), 2), &old_rows);
            drop((initial, obsolete));
            seed.root.clone()
        };
        let chosen_root = SourceDirectory::capture_for_path(&label).unwrap();
        fs::rename(&named, &moved).unwrap();
        let foreign = ProjectionGenerationStore::open(&named).unwrap();
        let foreign_rows = members(&[("a", "foreign source")]);
        let foreign_head = stage(&foreign, begin("g1", None, 1), &foreign_rows);
        foreign.publish(&foreign_head).unwrap();
        let foreign_pending = stage(&foreign, begin("g2", Some("g1"), 2), &foreign_rows);
        let foreign_obsolete = stage(&foreign, begin("obsolete", Some("g1"), 2), &foreign_rows);
        let foreign_before = tree(&named);
        let store = finish_capture(label, chosen_root);
        assert!(!Arc::ptr_eq(&store.shared, &foreign.shared));
        let reader = store.open_active("session_spans", b"session-1").unwrap();
        let next_rows = members(&[("a", "new original source")]);
        let next = stage(&store, begin("g2", Some("g1"), 2), &next_rows);
        store.publish(&next).unwrap();
        reader.scrub().unwrap();
        assert_eq!(
            reader
                .read_page(None, ProjectionGenerationReadLimits::default())
                .unwrap()
                .members,
            old_rows
        );
        let active = store.open_active("session_spans", b"session-1").unwrap();
        active.scrub().unwrap();
        assert_eq!(
            active
                .read_page(None, ProjectionGenerationReadLimits::default())
                .unwrap()
                .members,
            next_rows
        );
        let report = store
            .reclaim(ProjectionGenerationGcLimits::default())
            .unwrap();
        assert_eq!(report.generations_reclaimed, 1);
        let obsolete_data = store.data_path(&begin("obsolete", Some("g1"), 2).identity);
        assert!(!moved
            .join("generations")
            .join(obsolete_data.file_name().unwrap())
            .exists());
        assert_eq!(
            store.status(&next.manifest.begin.identity).unwrap().state,
            ProjectionGenerationState::Published
        );
        assert!(store.published_receipt(&next).unwrap().is_some());
        assert!(matches!(
            foreign.publish(&next),
            Err(ProjectionGenerationError::Conflict(_))
        ));
        assert_eq!(tree(&named), foreign_before);
        drop((
            reader,
            active,
            next,
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
fn repair_after_constructor_label_replacement_never_truncates_b() {
    let fixture = Fixture::new();
    let named = fixture.0.join("named");
    let moved = fixture.0.join("moved");
    let rows = members(&[("a", "first"), ("b", "second")]);
    let label = {
        let seed = ProjectionGenerationStore::open(&named).unwrap();
        let mut writer = seed
            .begin_candidate(
                begin("g1", None, 1),
                ProjectionGenerationBatchLimits::default(),
            )
            .unwrap();
        writer.append_batch(&rows[..1]).unwrap();
        let data = seed.data_path(&writer.begin.identity);
        drop(writer);
        let mut file = OpenOptions::new().append(true).open(data).unwrap();
        file.write_all(&[4, 0]).unwrap();
        file.sync_all().unwrap();
        seed.root.clone()
    };
    let chosen_root = SourceDirectory::capture_for_path(&label).unwrap();
    fs::rename(&named, &moved).unwrap();
    let foreign = ProjectionGenerationStore::open(&named).unwrap();
    let foreign_rows = members(&[("a", "foreign complete data")]);
    let foreign_head = stage(&foreign, begin("g1", None, 1), &foreign_rows);
    foreign.publish(&foreign_head).unwrap();
    let foreign_before = tree(&named);
    let store = finish_capture(label, chosen_root);
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
fn an_identical_b_cannot_adopt_a_token_created_from_the_chosen_a_root() {
    let fixture = Fixture::new();
    let named = fixture.0.join("named");
    let moved = fixture.0.join("moved");
    let rows = members(&[("a", "identical bytes")]);
    let label = {
        let seed = ProjectionGenerationStore::open(&named).unwrap();
        drop(stage(&seed, begin("g1", None, 1), &rows));
        seed.root.clone()
    };
    let bytes = tree(&named);
    let chosen_root = SourceDirectory::capture_for_path(&label).unwrap();
    fs::rename(&named, &moved).unwrap();
    fs::create_dir(&named).unwrap();
    for directory in ["heads", "generations"] {
        fs::create_dir(named.join(directory)).unwrap();
    }
    for (name, bytes) in &bytes {
        fs::write(named.join(name), bytes).unwrap();
    }
    let foreign = ProjectionGenerationStore::open(&named).unwrap();
    let store = finish_capture(label, chosen_root);
    let original = store
        .begin_candidate(
            begin("g1", None, 1),
            ProjectionGenerationBatchLimits::default(),
        )
        .unwrap()
        .seal(seal_for(&rows))
        .unwrap();
    assert!(!Arc::ptr_eq(&store.shared, &foreign.shared));
    assert!(matches!(
        foreign.publish(&original),
        Err(ProjectionGenerationError::Conflict(_))
    ));
    assert_eq!(tree(&named), bytes);
    store.publish(&original).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_refused_post_open_move_preserves_custody_data_and_publication() {
    let fixture = Fixture::new();
    let project = ProjectFileDescriptors::acquire(&fixture.0, 32).unwrap();
    let named = fixture.0.join("named");
    let destination = fixture.0.join("destination");
    let store = ProjectionGenerationStore::open(&named).unwrap();
    let identity = store.captured.identity();
    let rows = members(&[("a", "retained data")]);
    let initial = stage(&store, begin("g1", None, 1), &rows);
    store.publish(&initial).unwrap();
    drop(initial);
    let reader = store.open_active("session_spans", b"session-1").unwrap();
    let before = tree(&named);
    let error = fs::rename(&named, &destination).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(5));
    assert!(!destination.exists());
    assert_eq!(tree(&named), before);
    assert_eq!(store.captured.identity(), identity);
    assert_eq!(project.metrics().open, 3);
    let next = stage(
        &store,
        begin("g2", Some("g1"), 2),
        &members(&[("a", "new data")]),
    );
    store.publish(&next).unwrap();
    reader.scrub().unwrap();
    assert_eq!(
        reader
            .read_page(None, ProjectionGenerationReadLimits::default())
            .unwrap()
            .members,
        rows
    );
    drop((reader, next, store));
    assert_eq!(project.metrics().open, 0);
}

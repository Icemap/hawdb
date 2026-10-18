// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use std::io;
use std::path::Path;

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn native(source: &Path, destination: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let source = CString::new(source.as_os_str().as_bytes())?;
    let destination = CString::new(destination.as_os_str().as_bytes())?;
    // SAFETY: both C strings remain live for the synchronous native call. The
    // exclusive flag makes absence and mutation one filesystem operation.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    // SAFETY: same argument lifetime and no-replace contract as above.
    #[cfg(target_os = "macos")]
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn wide_absolute(path: &Path) -> io::Result<Vec<u16>> {
    use std::os::windows::ffi::OsStrExt;
    if path.as_os_str().encode_wide().any(|unit| unit == 0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory move path contains NUL",
        ));
    }
    let path = std::path::absolute(path)?;
    let mut result: Vec<u16> = path.as_os_str().encode_wide().collect();
    // GetFullPathName/absolute resolves relative components before using the
    // verbatim prefix, preserving long local and UNC names without MAX_PATH.
    const VERBATIM: [u16; 4] = [92, 92, 63, 92];
    if !result.starts_with(&VERBATIM) {
        if result.starts_with(&[92, 92]) {
            result.splice(..2, "\\\\?\\UNC\\".encode_utf16());
        } else {
            result.splice(..0, VERBATIM);
        }
    }
    result.push(0);
    Ok(result)
}

#[cfg(windows)]
pub(super) fn native(source: &Path, destination: &Path) -> io::Result<()> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "MoveFileW"]
        fn move_file(source: *const u16, destination: *const u16) -> i32;
    }
    let source = wide_absolute(source)?;
    let destination = wide_absolute(destination)?;
    // SAFETY: NUL-terminated UTF-16 buffers live through the synchronous call.
    // MoveFileW rejects an existing destination and cannot copy directories
    // across volumes. No replace or copy/delete fallback is requested.
    if unsafe { move_file(source.as_ptr(), destination.as_ptr()) } != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(super) fn native(_source: &Path, _destination: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace directory move is unavailable",
    ))
}

#[cfg(test)]
type MoveHook = std::cell::RefCell<Option<Box<dyn FnOnce()>>>;
#[cfg(test)]
thread_local! {
    static BEFORE_NATIVE_MOVE: MoveHook = const { std::cell::RefCell::new(None) };
}
#[cfg(test)]
pub(super) fn after_destination_check() {
    BEFORE_NATIVE_MOVE.with(|hook| {
        let callback = hook.borrow_mut().take();
        if let Some(callback) = callback {
            callback();
        }
    });
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos", windows)))]
mod tests {
    use crate::file_descriptors::ProjectFileDescriptors;
    use crate::file_io as fs;
    use std::io;
    use std::path::PathBuf;
    use std::sync::{Arc, Barrier};
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "hawdb-rename-new-{}",
                hawdb_core::generate_uuidv7().unwrap()
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn existing_empty_populated_and_file_targets_remain_unchanged() {
        let fixture = Fixture::new();
        let _project = ProjectFileDescriptors::acquire(&fixture.0, 16).unwrap();
        for kind in [0, 1, 2] {
            let source = fixture.0.join(format!("source-{kind}"));
            let target = fixture.0.join(format!("target-{kind}"));
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("source.hawdb"), b"source").unwrap();
            if kind == 2 {
                std::fs::write(&target, b"foreign").unwrap();
            } else {
                std::fs::create_dir(&target).unwrap();
                if kind == 1 {
                    std::fs::write(target.join("foreign.hawdb"), b"foreign").unwrap();
                }
            }
            assert!(fs::rename_directory_new(&source, &target).is_err());
            assert_eq!(
                std::fs::read(source.join("source.hawdb")).unwrap(),
                b"source"
            );
            match kind {
                0 => assert!(std::fs::read_dir(&target).unwrap().next().is_none()),
                1 => assert_eq!(
                    std::fs::read(target.join("foreign.hawdb")).unwrap(),
                    b"foreign"
                ),
                _ => assert_eq!(std::fs::read(&target).unwrap(), b"foreign"),
            }
        }
    }
    #[test]
    fn concurrent_publishers_have_one_winner_and_preserve_the_loser() {
        let fixture = Fixture::new();
        let _project = ProjectFileDescriptors::acquire(&fixture.0, 16).unwrap();
        let target = fixture.0.join("target");
        let gate = Arc::new(Barrier::new(3));
        let mut children = Vec::new();
        for id in 0..2 {
            let source = fixture.0.join(format!("source-{id}"));
            std::fs::create_dir(&source).unwrap();
            std::fs::write(source.join("id.hawdb"), [id]).unwrap();
            let target = target.clone();
            let gate = gate.clone();
            children.push(std::thread::spawn(move || {
                gate.wait();
                (
                    id,
                    source.clone(),
                    fs::rename_directory_new(&source, &target),
                )
            }));
        }
        gate.wait();
        let results: Vec<_> = children
            .into_iter()
            .map(|child| child.join().unwrap())
            .collect();
        assert_eq!(
            results
                .iter()
                .filter(|(_, _, result)| result.is_ok())
                .count(),
            1
        );
        let (winner, source, _) = results
            .iter()
            .find(|(_, _, result)| result.is_ok())
            .unwrap();
        assert!(!source.exists());
        assert_eq!(std::fs::read(target.join("id.hawdb")).unwrap(), [*winner]);
        let (loser, source, _) = results
            .iter()
            .find(|(_, _, result)| result.is_err())
            .unwrap();
        assert_eq!(std::fs::read(source.join("id.hawdb")).unwrap(), [*loser]);
    }
    #[test]
    fn a_target_created_after_the_check_is_not_replaced() {
        use std::cell::RefCell;
        use std::rc::Rc;
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, 16).unwrap();
        let source = fixture.0.join("source");
        let target = fixture.0.join("target");
        std::fs::create_dir(&source).unwrap();
        std::fs::write(source.join("source.hawdb"), b"source").unwrap();
        let saved = Rc::new(RefCell::new(None));
        let held = saved.clone();
        let created = target.clone();
        let context = project.io_context();
        super::BEFORE_NATIVE_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                std::fs::create_dir(&created).unwrap();
                *held.borrow_mut() =
                    Some(fs::SourceDirectory::capture(&created, &context).unwrap());
            }))
        });
        assert!(fs::rename_directory_new(&source, &target).is_err());
        assert_eq!(
            std::fs::read(source.join("source.hawdb")).unwrap(),
            b"source"
        );
        let current = fs::SourceDirectory::capture(&target, &project.io_context()).unwrap();
        assert!(saved.borrow().as_ref().unwrap().same_directory(&current));
        assert!(std::fs::read_dir(&target).unwrap().next().is_none());
    }

    #[test]
    fn recorded_exclusive_move_keeps_native_and_recovery_images_in_agreement() {
        use crate::power_loss::image::ImageLimits;
        use crate::power_loss::PowerLossModel;
        use std::io::Write;
        use std::path::Path;
        let fixture = Fixture::new();
        let project = ProjectFileDescriptors::acquire(&fixture.0, 32).unwrap();
        let model = PowerLossModel::attach(&project, ImageLimits::default()).unwrap();
        let source = fixture.0.join("source");
        let target = fixture.0.join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        let mut file = fs::File::create(source.join("data.hawdb")).unwrap();
        file.write_all(b"durable source").unwrap();
        file.sync_all().unwrap();
        drop(file);
        crate::durability::sync_directory(&source).unwrap();
        crate::durability::sync_directory(&fixture.0).unwrap();
        assert_eq!(
            fs::rename_directory_new(&source, &target)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        let snapshot = model.capture().unwrap();
        let image = snapshot.crash(&snapshot.persist_all_plan()).unwrap();
        assert_eq!(
            image.bytes(Path::new("source/data.hawdb")).unwrap(),
            b"durable source"
        );
        assert!(image.bytes(Path::new("target/data.hawdb")).is_none());
        fs::remove_dir(&target).unwrap();
        fs::rename_directory_new(&source, &target).unwrap();
        crate::durability::sync_directory(&fixture.0).unwrap();
        let snapshot = model.capture().unwrap();
        let image = snapshot.crash(&snapshot.persist_all_plan()).unwrap();
        assert!(image.bytes(Path::new("source/data.hawdb")).is_none());
        assert_eq!(
            image.bytes(Path::new("target/data.hawdb")).unwrap(),
            b"durable source"
        );
    }

    // Exercise the retained resources missing from empty-directory move tests.
    // A refused move must retain custody; dropping it to make a move succeed
    // would open a peer-acquisition window. Consumer relocation remains a
    // separate availability requirement and is not waived by this probe.
    #[test]
    fn open_child_custody_survives_native_move_or_refusal() {
        use crate::file_io::{SourceDirectory, SourceDirectoryLockMode};
        use std::fs::TryLockError;
        for standard_move in [false, true] {
            for resources in [0, 1, 2, 3, 4, 5] {
                let fixture = Fixture::new();
                let project = ProjectFileDescriptors::acquire(&fixture.0, 16).unwrap();
                let source = fixture.0.join("source");
                let destination = fixture.0.join("destination");
                std::fs::create_dir(&source).unwrap();
                std::fs::write(
                    source.join(".search-source-process.lock"),
                    b"retained child",
                )
                .unwrap();
                let directory = (resources != 0)
                    .then(|| SourceDirectory::capture(&source, &project.io_context()).unwrap());
                let locked = matches!(resources, 3 | 5);
                let child = (resources >= 2).then(|| {
                    if resources >= 4 {
                        return fs::OpenOptions::new()
                            .read(true)
                            .write(true)
                            .open(source.join(".search-source-process.lock"))
                            .unwrap();
                    }
                    directory
                        .as_ref()
                        .unwrap()
                        .open_lock_file(
                            &project.io_context(),
                            ".search-source-process.lock",
                            SourceDirectoryLockMode::ReadWriteCreate,
                        )
                        .unwrap()
                });
                if locked {
                    child.as_ref().unwrap().try_lock().unwrap();
                }
                let moved = if standard_move {
                    std::fs::rename(&source, &destination)
                } else {
                    fs::rename_directory_new(&source, &destination)
                };
                eprintln!("directory move probe: std={standard_move}, resources={resources} (0=none, 1=root, 2=relative-child, 3=relative-locked-child, 4=absolute-child, 5=absolute-locked-child), result={moved:?}");
                if resources == 0 {
                    moved.as_ref().unwrap();
                }
                let observed = if moved.is_ok() {
                    assert!(!source.exists());
                    &destination
                } else {
                    assert!(!destination.exists());
                    &source
                };
                // Windows byte-range locks exclude reads through another
                // handle too. Inspect identity while held; verify bytes after
                // closure below, without unlocking early for a pathname read.
                if !locked {
                    assert_eq!(
                        std::fs::read(observed.join(".search-source-process.lock")).unwrap(),
                        b"retained child"
                    );
                }
                if let Some(directory) = &directory {
                    let current =
                        SourceDirectory::capture(observed, &project.io_context()).unwrap();
                    assert!(directory.same_directory(&current));
                    if locked {
                        let peer = directory
                            .open_lock_file(
                                &project.io_context(),
                                ".search-source-process.lock",
                                SourceDirectoryLockMode::ReadWriteCreate,
                            )
                            .unwrap();
                        assert!(matches!(peer.try_lock(), Err(TryLockError::WouldBlock)));
                    }
                }
                drop(child);
                drop(directory);
                assert_eq!(project.metrics().open, 0);
                if moved.is_err() {
                    fs::rename_directory_new(&source, &destination).unwrap();
                }
                assert_eq!(
                    std::fs::read(destination.join(".search-source-process.lock")).unwrap(),
                    b"retained child"
                );
            }
        }
    }

    #[test]
    fn same_target_is_not_a_successful_exclusive_move() {
        let fixture = Fixture::new();
        assert!(fs::rename_directory_new(&fixture.0, &fixture.0).is_err());
        assert!(fixture.0.is_dir());
    }
}

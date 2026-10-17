// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::OpenOptionsExt;

pub(super) fn open_root(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)
}

fn name_bytes(name: ChildName<'_>) -> [u8; 256] {
    let mut bytes = [0; 256];
    bytes[..name.0.len()].copy_from_slice(name.0.as_bytes());
    bytes
}

pub(super) fn open_child(root: &std::fs::File, name: ChildName<'_>) -> io::Result<std::fs::File> {
    let bytes = name_bytes(name);
    // SAFETY: root is borrowed live; validated ASCII name is NUL-terminated in
    // fixed scratch. Read-only open needs no mode argument. NONBLOCK prevents a
    // corrupt FIFO entry blocking before the caller checks regular-file type.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            bytes.as_ptr().cast(),
            libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful openat returned a fresh owned descriptor exactly once.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

pub(super) fn remove_child(
    root: &std::fs::File,
    name: ChildName<'_>,
    _context: &FileOpenContext,
) -> io::Result<()> {
    let bytes = name_bytes(name);
    // SAFETY: borrowed live root and NUL-terminated single-component name; flags
    // zero unlink a file entry and cannot recursively delete a directory.
    let status = unsafe { libc::unlinkat(root.as_raw_fd(), bytes.as_ptr().cast(), 0) };
    if status == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

pub(super) fn open_lock_file(
    root: &std::fs::File,
    name: ChildName<'_>,
    mode: SourceDirectoryLockMode,
) -> io::Result<std::fs::File> {
    let bytes = name_bytes(name);
    let access = match mode {
        SourceDirectoryLockMode::ReadExisting => libc::O_RDONLY,
        SourceDirectoryLockMode::ReadWriteCreate => libc::O_RDWR | libc::O_CREAT,
    };
    // SAFETY: validated NUL-terminated component, borrowed live directory,
    // valid open flags and permission mode. No truncation or symlink traversal.
    let fd = unsafe {
        libc::openat(
            root.as_raw_fd(),
            bytes.as_ptr().cast(),
            access | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: one fresh owned descriptor from successful openat.
    Ok(unsafe { std::fs::File::from_raw_fd(fd) })
}

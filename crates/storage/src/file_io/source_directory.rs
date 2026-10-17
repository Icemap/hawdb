// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Captured native directory authority; caller owns publication exclusion and
//! artifact registrations separately. No pathname is retained or consulted by
//! child operations. This primitive does not qualify GC, durability or memory.

use super::{File, FileBacking, NativeFile};
use crate::file_descriptors::{DescriptorKind, DescriptorPermit, FileOpenContext};
use std::io;
use std::path::Path;

#[cfg(unix)]
#[path = "source_directory/unix.rs"]
mod platform;
#[cfg(windows)]
#[path = "source_directory/windows.rs"]
mod platform;
#[cfg(not(any(unix, windows)))]
mod platform {
    use super::*;
    pub(super) fn open_root(_path: &Path) -> io::Result<std::fs::File> {
        Err(unsupported())
    }
    pub(super) fn open_child(
        _root: &std::fs::File,
        _name: ChildName<'_>,
    ) -> io::Result<std::fs::File> {
        Err(unsupported())
    }
    pub(super) fn remove_child(
        _root: &std::fs::File,
        _name: ChildName<'_>,
        _context: &FileOpenContext,
    ) -> io::Result<()> {
        Err(unsupported())
    }
    pub(super) fn open_lock_file(
        _root: &std::fs::File,
        _name: ChildName<'_>,
        _mode: SourceDirectoryLockMode,
    ) -> io::Result<std::fs::File> {
        Err(unsupported())
    }
    fn unsupported() -> io::Error {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "native source directory requires Unix or Windows",
        )
    }
}

/// Physical identity copied while a counted directory handle is live. A
/// registry must retain that handle while using the key; this is not a logical
/// incarnation token and must not be reused after all capturing handles close.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SourceDirectoryIdentity(crate::native_file_identity::NativeIdentity);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceDirectoryLockMode {
    ReadExisting,
    ReadWriteCreate,
}

/// Internal source authority for generated ASCII artifact names, not arbitrary
/// host paths. Native handles and their descriptor receipts stay together.
///
/// Capture and every method require the caller's I/O admission. This type does
/// not take publication ownership, register reader pins, synchronize deletion,
/// or authorize a complete source watermark. Power-loss harness contexts refuse
/// until directory-relative operations have a modeled namespace implementation.
#[derive(Debug)]
pub struct SourceDirectory {
    root: File,
    context: FileOpenContext,
    identity: SourceDirectoryIdentity,
}

impl SourceDirectory {
    /// Use an existing project context; no new registry/domain is created.
    pub fn capture(path: &Path, context: &FileOpenContext) -> io::Result<Self> {
        let _native = native_scope(context)?;
        let permit = context.acquire(DescriptorKind::Transient)?;
        let inner = platform::open_root(path).map_err(|error| context.map_open_error(error))?;
        if !inner.metadata()?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                "source root is not a directory",
            ));
        }
        let identity = SourceDirectoryIdentity(crate::native_file_identity::of_file(&inner)?);
        Ok(Self {
            identity,
            root: counted(inner, permit),
            // Keep the project domain, not a producer's reservation inventory.
            // Future operations supply their own fresh reserved capacity.
            context: context.without_inventory(),
        })
    }

    /// No I/O, allocation or native handle acquisition is performed. Keep the
    /// captured directory alive while looking up its physical registry key.
    pub fn identity(&self) -> SourceDirectoryIdentity {
        self.identity
    }

    pub fn same_directory(&self, other: &Self) -> bool {
        self.identity.0 == other.identity.0
    }

    /// Pure domain comparison; does not open a file or consume FD inventory.
    pub fn shares_project_context(&self, context: &FileOpenContext) -> bool {
        std::sync::Arc::ptr_eq(&self.context.state, &context.state)
    }

    /// Opens only a hidden `.lock` control file, never truncating an existing
    /// file. Lock acquisition/release and process-registry lifetime belong to
    /// the caller. ReadExisting supports a read-only root without creating data.
    pub fn open_lock_file(
        &self,
        context: &FileOpenContext,
        name: &str,
        mode: SourceDirectoryLockMode,
    ) -> io::Result<File> {
        self.require_context(context)?;
        let name = ChildName::new(name)?;
        if !name.0.starts_with('.') || !name.0.ends_with(".lock") || name.0.len() < 7 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid source ownership lock name",
            ));
        }
        let _native = native_scope(context)?;
        let permit = context.acquire(DescriptorKind::OwnershipLock)?;
        let inner = self
            .root
            .with_native(|root| platform::open_lock_file(root, name, mode))
            .map_err(|error| context.map_open_error(error))?;
        if !inner.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source lock is not a regular file",
            ));
        }
        Ok(counted(inner, permit))
    }

    /// Opens one regular file without following a child symlink/reparse point.
    /// The returned file owns an admitted native handle; it is not an evictable
    /// path-based cache reference. The caller controls its retention lifetime.
    /// A fresh operation context supplies its reserved FD capacity; the
    /// directory capture context is never reused for child admission.
    pub fn open_child(&self, context: &FileOpenContext, name: &str) -> io::Result<File> {
        self.require_context(context)?;
        let name = ChildName::new(name)?;
        let _native = native_scope(context)?;
        let permit = context.acquire(DescriptorKind::Transient)?;
        let inner = self
            .root
            .with_native(|root| platform::open_child(root, name))
            .map_err(|error| context.map_open_error(error))?;
        if !inner.metadata()?.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "source child is not a regular file",
            ));
        }
        Ok(counted(inner, permit))
    }

    /// Unlinks one entry relative to the captured root. The caller must exclude
    /// registration/publication and establish that no live reader needs it.
    /// Namespace durability and replay are separate obligations. The operation
    /// context supplies any extra native FD capacity needed for deletion.
    pub fn remove_child(&self, context: &FileOpenContext, name: &str) -> io::Result<()> {
        self.require_context(context)?;
        let name = ChildName::new(name)?;
        let _native = native_scope(context)?;
        self.root
            .with_native(|root| platform::remove_child(root, name, context))
            .map_err(|error| context.map_open_error(error))
    }

    fn require_context(&self, context: &FileOpenContext) -> io::Result<()> {
        if self.shares_project_context(context) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "source directory operation requires its project context",
            ))
        }
    }
}

fn counted(inner: std::fs::File, permit: DescriptorPermit) -> File {
    File {
        backing: FileBacking::Native(NativeFile {
            inner,
            permit,
            #[cfg(any(test, feature = "test-support"))]
            trace: None,
        }),
    }
}

#[derive(Clone, Copy)]
struct ChildName<'a>(&'a str);
impl<'a> ChildName<'a> {
    fn new(name: &'a str) -> io::Result<Self> {
        let name = Self(name);
        // Engine artifact names fit one portable component and fixed native
        // scratch. Reject traversal, streams, separators and NUL before I/O.
        if name.0.is_empty()
            || name.0.len() > 255
            || matches!(name.0, "." | "..")
            || !name
                .0
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid source artifact name",
            ));
        }
        Ok(name)
    }
}

struct NativeScope<'a> {
    #[cfg(any(test, feature = "test-support"))]
    _guard: std::sync::MutexGuard<'a, std::sync::Weak<crate::power_loss::ModelCore>>,
    #[cfg(not(any(test, feature = "test-support")))]
    _borrow: std::marker::PhantomData<&'a FileOpenContext>,
}
fn native_scope(context: &FileOpenContext) -> io::Result<NativeScope<'_>> {
    #[cfg(any(test, feature = "test-support"))]
    {
        // Hold registration exclusion for the operation, so enabling a harness
        // cannot race an unrecorded native namespace mutation.
        let guard = context
            .state
            .power_loss
            .lock()
            .map_err(|_| io::Error::other("power-loss context poisoned"))?;
        if guard.upgrade().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "directory-relative source I/O has no power-loss harness mapping",
            ));
        }
        Ok(NativeScope { _guard: guard })
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        let _ = context;
        Ok(NativeScope {
            _borrow: std::marker::PhantomData,
        })
    }
}

#[cfg(all(test, any(unix, windows)))]
mod tests;

// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use std::ffi::c_void;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};

#[repr(C)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: *mut u16,
}
#[repr(C)]
struct ObjectAttributes {
    length: u32,
    root_directory: *mut c_void,
    object_name: *mut UnicodeString,
    attributes: u32,
    security_descriptor: *mut c_void,
    security_quality_of_service: *mut c_void,
}
#[repr(C)]
struct IoStatusBlock {
    status_or_pointer: usize,
    information: usize,
}

#[link(name = "ntdll")]
unsafe extern "system" {
    #[link_name = "NtCreateFile"]
    fn nt_create_file(
        file_handle: *mut *mut c_void,
        desired_access: u32,
        object_attributes: *mut ObjectAttributes,
        io_status: *mut IoStatusBlock,
        allocation_size: *const i64,
        file_attributes: u32,
        share_access: u32,
        create_disposition: u32,
        create_options: u32,
        ea_buffer: *const c_void,
        ea_length: u32,
    ) -> i32;
    #[link_name = "RtlNtStatusToDosError"]
    fn status_to_error(status: i32) -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    #[link_name = "SetFileInformationByHandle"]
    fn set_file_information(
        file: *mut c_void,
        class: i32,
        information: *const c_void,
        bytes: u32,
    ) -> i32;
}

const SYNCHRONIZE: u32 = 0x0010_0000;
const READ_ATTRIBUTES: u32 = 0x80;
const READ_DATA: u32 = 1;
const DELETE: u32 = 0x0001_0000;
const SHARE_ALL: u32 = 7;
const NON_DIRECTORY: u32 = 0x40;
const SYNCHRONOUS_NONALERT: u32 = 0x20;
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

pub(super) fn open_root(path: &Path) -> io::Result<std::fs::File> {
    const BACKUP_SEMANTICS: u32 = 0x0200_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_ALL)
        .custom_flags(BACKUP_SEMANTICS)
        .open(path)
}

fn relative_open(
    root: &std::fs::File,
    name: ChildName<'_>,
    access: u32,
    create: bool,
) -> io::Result<std::fs::File> {
    let mut encoded = [0_u16; 256];
    for (slot, byte) in encoded.iter_mut().zip(name.0.bytes()) {
        *slot = u16::from(byte);
    }
    let length = u16::try_from(name.0.len() * 2).expect("validated ASCII name fits native scratch");
    let mut unicode = UnicodeString {
        length,
        maximum_length: length,
        buffer: encoded.as_mut_ptr(),
    };
    let mut attributes = ObjectAttributes {
        length: u32::try_from(std::mem::size_of::<ObjectAttributes>())
            .expect("native struct fits u32"),
        root_directory: root.as_raw_handle(),
        object_name: &mut unicode,
        // Exact component lookup; no Win32 drive/stream/path normalization.
        attributes: 0,
        security_descriptor: std::ptr::null_mut(),
        security_quality_of_service: std::ptr::null_mut(),
    };
    let mut status = IoStatusBlock {
        status_or_pointer: 0,
        information: 0,
    };
    let mut handle = std::ptr::null_mut();
    // SAFETY: all repr(C) buffers match winternl.h and outlive the synchronous
    // call. RootDirectory is borrowed live. A validated single ASCII component
    // cannot escape it; FILE_OPEN_IF is used only for mutable lock controls,
    // neither disposition truncates existing files, and reparse targets
    // are not followed. No callback, security descriptor or EA buffer is passed.
    let result = unsafe {
        nt_create_file(
            &mut handle,
            access | READ_ATTRIBUTES | SYNCHRONIZE,
            &mut attributes,
            &mut status,
            std::ptr::null(),
            0,
            SHARE_ALL,
            if create { 3 } else { 1 },
            NON_DIRECTORY | SYNCHRONOUS_NONALERT | OPEN_REPARSE_POINT,
            std::ptr::null(),
            0,
        )
    };
    if result < 0 {
        // SAFETY: pure documented NTSTATUS-to-Win32 error conversion.
        return Err(io::Error::from_raw_os_error(
            unsafe { status_to_error(result) } as i32,
        ));
    }
    if handle.is_null() || handle == std::ptr::without_provenance_mut(usize::MAX) {
        return Err(io::Error::other(
            "native source open returned an invalid handle",
        ));
    }
    // SAFETY: successful NtCreateFile transferred one fresh handle; this File
    // exclusively closes it, including validation and error paths below.
    let file = unsafe { std::fs::File::from_raw_handle(handle) };
    let metadata = file.metadata()?;
    if metadata.file_attributes() & ATTRIBUTE_REPARSE_POINT != 0 || !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "source child is not a regular non-reparse file",
        ));
    }
    Ok(file)
}

pub(super) fn open_child(root: &std::fs::File, name: ChildName<'_>) -> io::Result<std::fs::File> {
    relative_open(root, name, READ_DATA, false)
}

pub(super) fn remove_child(
    root: &std::fs::File,
    name: ChildName<'_>,
    context: &FileOpenContext,
) -> io::Result<()> {
    // Windows deletes by a handle opened relative to RootDirectory. Reserve
    // this extra handle before opening it, and close before returning capacity.
    let _permit = context.acquire(DescriptorKind::Transient)?;
    let file = relative_open(root, name, DELETE, false)?;
    // FILE_DISPOSITION_INFO contains one Win32 BOOL, not a C++/Rust bool.
    let delete_file: i32 = 1;
    // SAFETY: class 4 is FileDispositionInfo and the repr(C)-equivalent BOOL
    // buffer is exactly four bytes; the borrowed DELETE-capable handle is live.
    let success = unsafe {
        set_file_information(
            file.as_raw_handle(),
            4,
            std::ptr::from_ref(&delete_file).cast(),
            4,
        )
    };
    if success == 0 {
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
    const WRITE_DATA: u32 = 2;
    match mode {
        SourceDirectoryLockMode::ReadExisting => relative_open(root, name, READ_DATA, false),
        SourceDirectoryLockMode::ReadWriteCreate => {
            relative_open(root, name, READ_DATA | WRITE_DATA, true)
        }
    }
}

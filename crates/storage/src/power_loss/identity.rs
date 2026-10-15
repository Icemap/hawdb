// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs::{File, Metadata};
use std::io;

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NativeIdentity {
    volume: u64,
    file: [u8; 16],
}

pub(super) fn require_direct_entry(metadata: &Metadata) -> io::Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(super::invalid(
            "symlinks are outside the captured fault model",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(super::invalid(
                "reparse points are outside the captured fault model",
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
pub(super) fn of_file(file: &File) -> io::Result<NativeIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata()?;
    Ok(NativeIdentity {
        volume: metadata.dev(),
        file: u128::from(metadata.ino()).to_le_bytes(),
    })
}

#[cfg(windows)]
pub(super) fn of_file(file: &File) -> io::Result<NativeIdentity> {
    use std::ffi::c_void;
    use std::os::windows::io::AsRawHandle;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        #[link_name = "GetFileInformationByHandleEx"]
        fn get_file_information_by_handle_ex(
            file: *mut c_void,
            class: i32,
            information: *mut c_void,
            size: u32,
        ) -> i32;
    }
    // FILE_ID_INFO combines a volume serial with the full 128-bit file ID.
    // A truncated BY_HANDLE_FILE_INFORMATION ID is not unique on ReFS.
    const FILE_ID_INFO: i32 = 18;
    let mut identity = NativeIdentity::default();
    // SAFETY: the borrowed file keeps the handle alive; repr(C) exactly matches
    // FILE_ID_INFO, and the output pointer and size describe its writable buffer.
    let success = unsafe {
        get_file_information_by_handle_ex(
            file.as_raw_handle(),
            FILE_ID_INFO,
            std::ptr::from_mut(&mut identity).cast(),
            std::mem::size_of::<NativeIdentity>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(identity)
}

#[cfg(not(any(unix, windows)))]
pub(super) fn of_file(_file: &File) -> io::Result<NativeIdentity> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "native identity requires Unix or Windows",
    ))
}

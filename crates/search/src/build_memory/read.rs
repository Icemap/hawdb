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

//! Reader metadata retains capacity without retaining operation execution.

use super::{checked_add, BuildMemory, ACCOUNT_METADATA_BYTES};
use crate::{HawDBError, Result};
use hawdb_core::{RuntimeCancellationReason, RuntimeIoWaveError, RuntimeTaskContext};
use hawdb_executor::QueryMemoryLease;
use hawdb_qos::{RuntimePermit, RuntimeRetainedMemory};

/// Internal resource ownership, independent of source/publication authority.
#[doc(hidden)]
#[derive(Debug)]
pub struct ReaderMemory {
    // The query lease's account reference must die before the host reservation.
    _query: QueryMemoryLease,
    _host: RuntimeRetainedMemory,
}

/// Borrow execution only while constructing a reader. Returned memory owners
/// carry neither this context, BuildMemory nor the active RuntimePermit.
pub(crate) struct ReadAdmission<'a> {
    pub(crate) memory: &'a BuildMemory,
    pub(crate) task: &'a RuntimeTaskContext,
    pub(crate) permit: &'a RuntimePermit,
}

impl ReadAdmission<'_> {
    pub(crate) fn retained(&self, bytes: usize) -> Result<ReaderMemory> {
        let query = self.memory.retained.reserve(bytes)?;
        let host_bytes = checked_add(bytes, ACCOUNT_METADATA_BYTES)?;
        let host_bytes = u64::try_from(host_bytes)
            .map_err(|_| HawDBError::Execution("reader capacity exceeds u64".into()))?;
        let host = self
            .permit
            .reserve_retained_memory(host_bytes)
            .map_err(|error| HawDBError::Execution(error.to_string()))?;
        Ok(ReaderMemory {
            _query: query,
            _host: host,
        })
    }

    pub(crate) fn scratch(&self, bytes: usize) -> Result<QueryMemoryLease> {
        self.memory.spool.reserve(bytes)
    }

    pub(crate) fn io(
        &self,
    ) -> std::result::Result<Option<Box<dyn hawdb_core::RuntimeIoWavePermit>>, RuntimeIoWaveError>
    {
        self.task.acquire_io_wave(std::num::NonZeroUsize::MIN)
    }

    pub(crate) fn checkpoint(&self) -> std::result::Result<(), RuntimeCancellationReason> {
        self.task.checkpoint()
    }
}

#[cfg(test)]
thread_local! {
    static AFTER_FILE_OPEN: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Encoded bytes die before the reservation that admitted their allocation.
pub(crate) struct ReadBytes<'execution> {
    // Scratch payload cannot escape the construction permit it borrows.
    _execution: std::marker::PhantomData<&'execution RuntimePermit>,
    value: Vec<u8>,
    _memory: Option<QueryMemoryLease>,
}

impl ReadBytes<'_> {
    pub(crate) fn unaccounted(value: Vec<u8>) -> Self {
        Self {
            value,
            _memory: None,
            _execution: std::marker::PhantomData,
        }
    }
}

impl std::ops::Deref for ReadBytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.value
    }
}

impl<'execution> ReadAdmission<'execution> {
    /// Validate and decode must consume this same held file, never a pathname
    /// inspected before a second open. Artifact bytes are transient ownership.
    pub(crate) fn read_bound(
        &self,
        path: &std::path::Path,
        expected_len: u64,
        expected_checksum: u64,
        maximum: u64,
    ) -> Result<ReadBytes<'execution>> {
        use super::reserved::native_path;
        use hawdb_storage::file_io::File;
        use std::io::Read;
        crate::build_control::checkpoint(self.task)?;
        if expected_len > maximum {
            return Err(HawDBError::Storage(
                "reader artifact exceeds its encoded limit".into(),
            ));
        }
        let length = usize::try_from(expected_len).map_err(|_| {
            HawDBError::Storage("reader artifact length exceeds address space".into())
        })?;
        let _native = self.scratch(checked_add(native_path::bytes(path)?, 4096)?)?;
        let memory = self.scratch(length)?;
        let _buffer = self.scratch(super::SPOOL_BUFFER_BYTES)?;
        // One admitted I/O scope covers cold identity validation and the
        // selected read. No logical metadata call may bypass that scope.
        let _io = self
            .io()
            .map_err(|e| HawDBError::Execution(e.to_string()))?;
        let file = File::open(path)?;
        #[cfg(test)]
        if let Some(callback) = AFTER_FILE_OPEN.with(|slot| slot.replace(None)) {
            callback();
        }
        crate::build_control::checkpoint(self.task)?;
        let mut control = FileReadControl {
            admission: self,
            expected_len,
            validation_memory: None,
            error: None,
        };
        let result = file.with_admitted_read(&mut control, |file| {
            Ok((|| -> Result<ReadBytes<'execution>> {
                if file.metadata()?.len() != expected_len {
                    return Err(HawDBError::Storage(
                        "reader artifact length mismatch".into(),
                    ));
                }
                let mut output = ReadBytes {
                    value: Vec::new(),
                    _memory: Some(memory),
                    _execution: std::marker::PhantomData,
                };
                output.value.try_reserve_exact(length).map_err(|error| {
                    HawDBError::Execution(format!("reader artifact allocation failed: {error}"))
                })?;
                if output.value.capacity() > length {
                    return Err(HawDBError::Execution(
                        "reader artifact allocation exceeds admission".into(),
                    ));
                }
                let mut buffer = [0; super::SPOOL_BUFFER_BYTES];
                let mut checksum = hawdb_integrity::Crc32cHasher::new();
                loop {
                    crate::build_control::checkpoint(self.task)?;
                    let read = match file.read(&mut buffer) {
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        result => result?,
                    };
                    if read == 0 {
                        break;
                    }
                    if read > length - output.value.len() {
                        return Err(HawDBError::Storage(
                            "reader artifact grew during read".into(),
                        ));
                    }
                    checksum.update(&buffer[..read]);
                    output.value.extend_from_slice(&buffer[..read]);
                }
                if file.metadata()?.len() != expected_len
                    || output.value.len() != length
                    || checksum.finish() != expected_checksum
                {
                    return Err(HawDBError::Storage(
                        "reader artifact length or checksum mismatch".into(),
                    ));
                }
                Ok(output)
            })())
        });
        crate::build_control::checkpoint(self.task)?;
        if let Some(error) = control.error.take() {
            return Err(error);
        }
        result?.0
    }
}

struct FileReadControl<'a, 'execution> {
    admission: &'a ReadAdmission<'execution>,
    expected_len: u64,
    validation_memory: Option<QueryMemoryLease>,
    error: Option<HawDBError>,
}

impl hawdb_storage::file_io::ReadAdmissionControl for FileReadControl<'_, '_> {
    fn admit_validation(&mut self, bytes: u64) -> std::io::Result<()> {
        let result = (|| {
            if bytes != self.expected_len {
                return Err(HawDBError::Storage(
                    "reader immutable identity length mismatch".into(),
                ));
            }
            self.validation_memory = Some(self.admission.scratch(64 << 10)?);
            Ok(())
        })();
        result.map_err(|error| {
            // Keep typed budget failures instead of flattening them through
            // the managed file's generic I/O result.
            self.error = Some(error);
            std::io::Error::other("reader validation admission refused")
        })
    }

    fn checkpoint(&mut self) -> std::io::Result<()> {
        crate::build_control::checkpoint(self.admission.task).map_err(std::io::Error::other)
    }
}

#[cfg(test)]
pub(crate) mod tests;

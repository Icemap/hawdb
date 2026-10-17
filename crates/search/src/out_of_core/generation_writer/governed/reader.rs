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

//! Same-governor retained capacity for privately opened vector artifacts.

use super::GovernedSearchGenerationWriter;
use crate::build_memory::{checked_add, ACCOUNT_METADATA_BYTES};
use crate::HawDBError;
use hawdb_vector_projection::{ProjectionError, ProjectionReadAdmission};

/// Resource ownership only; this value grants no C-ready or source authority.
#[doc(hidden)]
#[derive(Debug)]
pub struct ProjectionReaderMemory {
    // Shared ledger metadata must die before the final host reservation.
    _query: hawdb_executor::QueryMemoryLease,
    _host: hawdb_qos::RuntimeRetainedMemory,
}

impl ProjectionReadAdmission for GovernedSearchGenerationWriter {
    type Retained = ProjectionReaderMemory;
    type Scratch = hawdb_executor::QueryMemoryLease;
    type Io = Option<Box<dyn hawdb_core::RuntimeIoWavePermit>>;

    fn reserve_retained(&self, bytes: usize) -> hawdb_vector_projection::Result<Self::Retained> {
        let query = self
            .writer
            .memory
            .retained
            .reserve(bytes)
            .map_err(ProjectionError::Admission)?;
        // Conservative per-receipt coverage includes the fixed account ledger
        // retained by QueryMemoryLease. A complete reader can amortize this
        // shared metadata; never retain an active RuntimePermit in the output.
        let host_bytes =
            checked_add(bytes, ACCOUNT_METADATA_BYTES).map_err(ProjectionError::Admission)?;
        let host_bytes = u64::try_from(host_bytes).map_err(|_| {
            ProjectionError::Admission(HawDBError::Execution(
                "projection reader capacity exceeds u64".into(),
            ))
        })?;
        let host = self
            .admission
            .permit
            .reserve_retained_memory(host_bytes)
            .map_err(|error| {
                ProjectionError::Admission(HawDBError::Execution(error.to_string()))
            })?;
        Ok(ProjectionReaderMemory {
            _query: query,
            _host: host,
        })
    }

    fn reserve_scratch(&self, bytes: usize) -> hawdb_vector_projection::Result<Self::Scratch> {
        self.writer
            .memory
            .spool
            .reserve(bytes)
            .map_err(ProjectionError::Admission)
    }

    fn acquire_io(&self) -> hawdb_vector_projection::Result<Self::Io> {
        self.writer
            .task_context
            .acquire_io_wave(std::num::NonZeroUsize::MIN)
            .map_err(|error| match error {
                hawdb_core::RuntimeIoWaveError::Stopped(reason) => {
                    ProjectionError::Cancelled(reason)
                }
                error => ProjectionError::Admission(HawDBError::Execution(error.to_string())),
            })
    }

    fn checkpoint(&self) -> hawdb_vector_projection::Result<()> {
        self.writer
            .task_context
            .checkpoint()
            .map_err(ProjectionError::Cancelled)
    }
}

#[cfg(test)]
mod tests;

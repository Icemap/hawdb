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

//! Observe process capacity without modifying host-owned resource limits.

use hawdb_core::error::{FileDescriptorError, HawDBError};

#[derive(Debug, Clone, Copy)]
pub(super) struct Capacity {
    pub allowance: usize,
    pub soft: u64,
    pub hard: u64,
}

impl Capacity {
    pub fn rejection(self, requested: usize) -> FileDescriptorError {
        FileDescriptorError::OsLimit {
            requested,
            os_code: None,
            soft: Some(self.soft),
            hard: Some(self.hard),
        }
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub(super) const HOST_HEADROOM: usize = 64;

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod native {
    use super::{Capacity, HawDBError, HOST_HEADROOM};
    use hawdb_core::error::FileDescriptorError;
    use std::io;

    // Leave space for lock/WAL, manifest/publication and immutable reads. A
    // smaller explicit project quota remains supported for component fixtures.
    const MIN_PROJECT_CAPACITY: usize = 8;

    fn value<T: Into<u64>>(limit: T) -> u64 {
        limit.into()
    }

    fn rejected(limit: Option<libc::rlimit>, requested: usize, os_code: Option<i32>) -> HawDBError {
        HawDBError::FileDescriptors(FileDescriptorError::OsLimit {
            requested,
            os_code,
            soft: limit.as_ref().map(|limit| value(limit.rlim_cur)),
            hard: limit.as_ref().map(|limit| value(limit.rlim_max)),
        })
    }

    fn read() -> io::Result<libc::rlimit> {
        let mut limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        // SAFETY: limit is writable, correctly sized, and lives through the call.
        if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limit) } != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(limit)
    }

    pub(super) fn project_limits(budget: usize) -> Result<(usize, Option<Capacity>), HawDBError> {
        let minimum = budget.min(MIN_PROJECT_CAPACITY);
        let requested = minimum + HOST_HEADROOM;
        let current = read().map_err(|error| rejected(None, requested, error.raw_os_error()))?;
        let allowance = usize::try_from(value(current.rlim_cur))
            .unwrap_or(usize::MAX)
            .saturating_sub(HOST_HEADROOM);
        let effective = budget.min(allowance);
        if effective < minimum {
            return Err(rejected(Some(current), requested, None));
        }
        Ok((
            effective,
            Some(Capacity {
                allowance,
                soft: value(current.rlim_cur),
                hard: value(current.rlim_max),
            }),
        ))
    }

    pub(super) fn current_soft_limit() -> Option<u64> {
        read().ok().map(|limit| value(limit.rlim_cur))
    }
}

pub(super) fn project_limits(budget: usize) -> Result<(usize, Option<Capacity>), HawDBError> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        native::project_limits(budget)
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    {
        Ok((budget, None))
    }
}

pub(super) fn current_soft_limit() -> Option<u64> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        native::current_soft_limit()
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    None
}

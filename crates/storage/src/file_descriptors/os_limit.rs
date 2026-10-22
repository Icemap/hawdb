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

//! Share the observed process allowance without modifying host-owned limits.

use hawdb_core::error::{FileDescriptorError, HawDBError};

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub(super) const HOST_HEADROOM: usize = 64;

#[cfg(all(unix, not(target_arch = "wasm32")))]
mod native {
    use super::{FileDescriptorError, HawDBError, HOST_HEADROOM};
    use std::io;
    use std::sync::Mutex;

    const MIN_PROJECT_CAPACITY: usize = 8;

    #[derive(Clone, Copy)]
    struct Capacity {
        allowance: usize,
        soft: u64,
        hard: u64,
    }
    impl Capacity {
        fn new(limit: &libc::rlimit) -> Self {
            let soft = value(limit.rlim_cur);
            Self {
                allowance: usize::try_from(soft)
                    .unwrap_or(usize::MAX)
                    .saturating_sub(HOST_HEADROOM),
                soft,
                hard: value(limit.rlim_max),
            }
        }
        fn rejection(self, requested: usize) -> FileDescriptorError {
            FileDescriptorError::OsLimit {
                requested,
                os_code: None,
                soft: Some(self.soft),
                hard: Some(self.hard),
            }
        }
    }

    // One charge covers both unused reservation credit and its opened handle.
    // Transfers between them never change the process total.
    struct ProcessLedger {
        capacity: Option<Capacity>,
        used: usize,
    }
    static PROCESS: Mutex<ProcessLedger> = Mutex::new(ProcessLedger {
        capacity: None,
        used: 0,
    });

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

    pub(super) fn effective_limit(budget: usize) -> Result<usize, HawDBError> {
        let minimum = budget.min(MIN_PROJECT_CAPACITY);
        let requested = minimum + HOST_HEADROOM;
        let current = read().map_err(|error| rejected(None, requested, error.raw_os_error()))?;
        let capacity = Capacity::new(&current);
        let effective = budget.min(capacity.allowance);
        if effective < minimum {
            return Err(rejected(Some(current), requested, None));
        }
        // New project registration refreshes the shared observation without
        // discarding any already charged handles or prepaid credit.
        PROCESS.lock().unwrap_or_else(|p| p.into_inner()).capacity = Some(capacity);
        Ok(effective)
    }

    pub(super) fn reserve(requested: usize) -> Result<(), (FileDescriptorError, usize)> {
        if requested == 0 {
            return Ok(());
        }
        let mut process = PROCESS.lock().unwrap_or_else(|p| p.into_inner());
        let capacity = match process.capacity {
            Some(capacity) => capacity,
            None => {
                let current = read().map_err(|error| {
                    (
                        FileDescriptorError::OsLimit {
                            requested,
                            os_code: error.raw_os_error(),
                            soft: None,
                            hard: None,
                        },
                        requested,
                    )
                })?;
                let capacity = Capacity::new(&current);
                process.capacity = Some(capacity);
                capacity
            }
        };
        let total = process
            .used
            .checked_add(requested)
            .ok_or_else(|| (capacity.rejection(requested), usize::MAX))?;
        if total > capacity.allowance {
            return Err((capacity.rejection(requested), total - capacity.allowance));
        }
        process.used = total;
        Ok(())
    }

    pub(super) fn release(released: usize) {
        if released == 0 {
            return;
        }
        let mut process = PROCESS.lock().unwrap_or_else(|p| p.into_inner());
        process.used = process
            .used
            .checked_sub(released)
            .expect("process descriptor credit released exactly once");
    }

    pub(super) fn current_soft_limit() -> Option<u64> {
        read().ok().map(|limit| value(limit.rlim_cur))
    }
}

pub(super) fn effective_limit(budget: usize) -> Result<usize, HawDBError> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        native::effective_limit(budget)
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    {
        Ok(budget)
    }
}

pub(super) fn reserve(requested: usize) -> Result<(), (FileDescriptorError, usize)> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        native::reserve(requested)
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    {
        let _ = requested;
        Ok(())
    }
}

pub(super) fn release(released: usize) {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    native::release(released);
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    let _ = released;
}

pub(super) fn current_soft_limit() -> Option<u64> {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        native::current_soft_limit()
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    None
}

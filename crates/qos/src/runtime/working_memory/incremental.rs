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

//! Allocation-sized governor/process charges, independent of execution slots.

use super::*;

#[derive(Debug)]
struct IncrementalMemoryPermit {
    controller: Arc<GovernorMemoryController>,
    bytes: u64,
    memory: RuntimeRetainedMemory,
}

impl GovernorMemoryController {
    pub(super) fn reserve_incremental(
        self: &Arc<Self>,
        bytes: u64,
        ceiling: u64,
    ) -> Result<Box<dyn RuntimeMemoryPermit>, RuntimeMemoryError> {
        let mut state = mutex_lock(&self.state);
        if !state.active {
            return Err(RuntimeMemoryError::Closed);
        }
        let ceiling = self.ceiling.min(ceiling);
        let available = ceiling.saturating_sub(state.charged_bytes);
        // The allocation lease itself is heap allocated. Admit its concrete
        // payload before creating it; allocator rounding remains a platform
        // assumption. The fixed owner charge covers the shared controller.
        let charged = bytes
            .checked_add(std::mem::size_of::<IncrementalMemoryPermit>() as u64)
            .ok_or(RuntimeMemoryError::AdmissionDenied {
                requested_bytes: u64::MAX,
                available_bytes: available,
                retryable: false,
            })?;
        if charged > available {
            return Err(RuntimeMemoryError::AdmissionDenied {
                requested_bytes: charged,
                available_bytes: available,
                retryable: charged <= ceiling,
            });
        }
        let memory = self
            .governor
            .reserve_retained_memory_with_floor(
                RuntimeWorkPriority::Background,
                charged,
                state
                    .reservation
                    .as_ref()
                    .expect("active incremental controller retains its owner")
                    .bytes,
            )
            .map_err(|error| RuntimeMemoryError::AdmissionDenied {
                requested_bytes: error.requested,
                available_bytes: error.available,
                retryable: error.retryable,
            })?;
        state.charged_bytes += charged;
        state.peak_charged_bytes = state.peak_charged_bytes.max(state.charged_bytes);
        drop(state);
        Ok(Box::new(IncrementalMemoryPermit {
            controller: self.clone(),
            bytes,
            memory,
        }))
    }
}

impl RuntimeMemoryPermit for IncrementalMemoryPermit {
    fn bytes(&self) -> u64 {
        self.bytes
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn merge(
        &mut self,
        mut incoming: Box<dyn RuntimeMemoryPermit>,
    ) -> Result<(), Box<dyn RuntimeMemoryPermit>> {
        let Some(source) = incoming
            .as_any_mut()
            .and_then(|value| value.downcast_mut::<Self>())
        else {
            return Err(incoming);
        };
        if !Arc::ptr_eq(&self.controller, &source.controller) {
            return Err(incoming);
        }
        let Some(bytes) = self.bytes.checked_add(source.bytes) else {
            return Err(incoming);
        };
        if !self.memory.merge(&mut source.memory) {
            return Err(incoming);
        }
        self.bytes = bytes;
        source.bytes = 0;
        drop(incoming);
        // The redundant concrete permit is gone. Refund its payload from the
        // controller and governor; partially observed RSS stays conservative.
        let released = std::mem::size_of::<Self>() as u64;
        {
            let mut state = mutex_lock(&self.controller.state);
            state.charged_bytes -= released;
        }
        self.memory.shrink_to(self.memory.bytes() - released);
        Ok(())
    }
}

impl Drop for IncrementalMemoryPermit {
    fn drop(&mut self) {
        let mut state = mutex_lock(&self.controller.state);
        state.charged_bytes -= self.memory.bytes();
        // The fixed owner charge belongs to the shared controller itself,
        // including closed task clones. Its final Arc drop refunds that charge.
        // Release this lock before the memory field wakes admission waiters.
    }
}

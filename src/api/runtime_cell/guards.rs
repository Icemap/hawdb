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

use super::*;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

/// Frontend execution owns this bundle through lifetime guards. The owner may adopt a
/// durable candidate while holding Control and acquiring this write guard
/// without waiting. Read guards retain the bundle until their last borrow.
#[derive(Debug, Default)]
pub(in crate::api) struct SharedBranchRuntime {
    value: RwLock<Option<Box<AdmittedBranchRuntime>>>,
}

impl SharedBranchRuntime {
    pub(in crate::api) fn new(runtime: Option<AdmittedBranchRuntime>) -> Self {
        Self {
            value: RwLock::new(runtime.map(Box::new)),
        }
    }

    pub(in crate::api) fn read<'a>(
        &'a self,
        control: &'a Control,
    ) -> Result<Option<RuntimeRead<'a>>> {
        let value = self.value.read().map_err(|_| poisoned())?;
        Ok(value.is_some().then_some(RuntimeRead {
            value: Some(value),
            control,
        }))
    }

    pub(in crate::api) fn try_read<'a>(
        &'a self,
        control: &'a Control,
    ) -> Result<Option<RuntimeRead<'a>>> {
        match self.value.try_read() {
            Ok(value) => Ok(value.is_some().then_some(RuntimeRead {
                value: Some(value),
                control,
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err(poisoned()),
        }
    }

    pub(in crate::api) fn initialize(&self, runtime: AdmittedBranchRuntime) -> Result<()> {
        let mut value = self.value.write().map_err(|_| poisoned())?;
        if value.is_some() {
            return Err(HawDBError::StorageIntegrity(
                "branch runtime was concurrently published twice".into(),
            ));
        }
        *value = Some(Box::new(runtime));
        Ok(())
    }

    pub(in crate::api) fn write<'a>(&'a self, control: &'a Control) -> Result<RuntimeWrite<'a>> {
        self.ready(self.value.write().map_err(|_| poisoned())?, control)
    }

    pub(in crate::api) fn try_write<'a>(
        &'a self,
        control: &'a Control,
    ) -> Result<Option<RuntimeWrite<'a>>> {
        match self.value.try_write() {
            Ok(value) => self.ready(value, control).map(Some),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err(poisoned()),
        }
    }

    fn ready<'a>(
        &self,
        value: RwLockWriteGuard<'a, Option<Box<AdmittedBranchRuntime>>>,
        control: &'a Control,
    ) -> Result<RuntimeWrite<'a>> {
        if value.is_none() {
            return Err(HawDBError::StorageIntegrity(
                "completed branch admission has no runtime".into(),
            ));
        }
        Ok(RuntimeWrite {
            value: Some(value),
            control,
        })
    }

    pub(in crate::api) fn take(&self) -> Result<Option<Box<AdmittedBranchRuntime>>> {
        Ok(self.value.write().map_err(|_| poisoned())?.take())
    }
}

fn poisoned() -> HawDBError {
    HawDBError::StorageIntegrity("branch runtime lock is poisoned".into())
}

#[derive(Debug)]
pub(in crate::api) struct RuntimeRead<'a> {
    value: Option<RwLockReadGuard<'a, Option<Box<AdmittedBranchRuntime>>>>,
    control: &'a Control,
}

impl Deref for RuntimeRead<'_> {
    type Target = AdmittedBranchRuntime;
    fn deref(&self) -> &Self::Target {
        self.value
            .as_ref()
            .expect("live runtime guard")
            .as_deref()
            .expect("admitted runtime")
    }
}

impl Drop for RuntimeRead<'_> {
    fn drop(&mut self) {
        // Release the borrow before waking the publisher. A read never waits
        // for Control, including when another caller deliberately holds it.
        drop(self.value.take());
        self.control.wake_handoff();
    }
}

#[derive(Debug)]
pub(in crate::api) struct RuntimeWrite<'a> {
    value: Option<RwLockWriteGuard<'a, Option<Box<AdmittedBranchRuntime>>>>,
    control: &'a Control,
}

impl Deref for RuntimeWrite<'_> {
    type Target = AdmittedBranchRuntime;
    fn deref(&self) -> &Self::Target {
        self.value
            .as_ref()
            .expect("live runtime guard")
            .as_deref()
            .expect("admitted runtime")
    }
}

impl DerefMut for RuntimeWrite<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
            .as_mut()
            .expect("live runtime guard")
            .as_deref_mut()
            .expect("admitted runtime")
    }
}

impl Drop for RuntimeWrite<'_> {
    fn drop(&mut self) {
        drop(self.value.take());
        self.control.wake_handoff();
    }
}

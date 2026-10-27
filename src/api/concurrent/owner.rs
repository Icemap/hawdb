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

use super::super::Database;
use crate::{HawDBError, Result};
use std::fmt::Debug;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

pub(crate) type SharedDatabaseGuard<'a> = Box<dyn DerefMut<Target = Database> + 'a>;

/// Borrows the same canonical owner used by projection publication and maintenance.
/// A guard must preserve publication/poison handling until its database borrow ends.
pub(crate) trait SharedDatabaseOwner: Debug + Send + Sync {
    fn lock(&self) -> Result<SharedDatabaseGuard<'_>>;
    fn try_lock(&self) -> Result<Option<SharedDatabaseGuard<'_>>>;
    fn is_poisoned(&self) -> bool;
    fn read_view(&self) -> Result<Arc<super::publication::PublishedConcurrentRead>>;
}

pub(super) enum DatabaseOwner {
    Direct(Box<Mutex<Database>>),
    Shared(Arc<dyn SharedDatabaseOwner>),
}

impl DatabaseOwner {
    pub(super) fn lock(&self) -> Result<DatabaseOwnerGuard<'_>> {
        match self {
            Self::Direct(database) => database
                .lock()
                .map(DatabaseOwnerGuard::Direct)
                .map_err(|_| poisoned()),
            Self::Shared(owner) => owner.lock().map(DatabaseOwnerGuard::Shared),
        }
    }

    pub(super) fn try_lock(&self) -> Result<Option<DatabaseOwnerGuard<'_>>> {
        match self {
            Self::Direct(database) => match database.try_lock() {
                Ok(guard) => Ok(Some(DatabaseOwnerGuard::Direct(guard))),
                Err(TryLockError::WouldBlock) => Ok(None),
                Err(TryLockError::Poisoned(_)) => Err(poisoned()),
            },
            Self::Shared(owner) => owner
                .try_lock()
                .map(|guard| guard.map(DatabaseOwnerGuard::Shared)),
        }
    }

    pub(super) fn is_poisoned(&self) -> bool {
        match self {
            Self::Direct(database) => database.is_poisoned(),
            Self::Shared(owner) => owner.is_poisoned(),
        }
    }
}

/// The ordinary concurrent facade keeps its allocation-free native mutex guard.
/// Shared publication owners erase only the host guard, not transaction logic.
pub(super) enum DatabaseOwnerGuard<'a> {
    Direct(MutexGuard<'a, Database>),
    Shared(SharedDatabaseGuard<'a>),
}

impl Deref for DatabaseOwnerGuard<'_> {
    type Target = Database;

    fn deref(&self) -> &Database {
        match self {
            Self::Direct(guard) => guard,
            Self::Shared(guard) => guard,
        }
    }
}

impl DatabaseOwnerGuard<'_> {
    pub(super) fn publishes_on_drop(&self) -> bool {
        matches!(self, Self::Shared(_))
    }
}

impl DerefMut for DatabaseOwnerGuard<'_> {
    fn deref_mut(&mut self) -> &mut Database {
        match self {
            Self::Direct(guard) => guard,
            Self::Shared(guard) => guard,
        }
    }
}

fn poisoned() -> HawDBError {
    HawDBError::Execution("concurrent commit sequencer is poisoned".into())
}

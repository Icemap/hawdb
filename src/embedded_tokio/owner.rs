// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::{ensure_autocommit, HawDBEmbedded};
use crate::api::{PublishedConcurrentRead, SharedDatabaseGuard, SharedDatabaseOwner};
use crate::{ConcurrentDatabase, Database, DatabaseInstanceIdentity, HawDBError, Result};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

#[derive(Debug)]
pub(super) struct EmbeddedOwner {
    state: Mutex<HawDBEmbedded>,
    published: Mutex<Option<Arc<PublishedConcurrentRead>>>,
    failed: AtomicBool,
    user_transaction: AtomicBool,
    // Ordinary writers share this gate. Direct host mutation/context replacement
    // needs its exclusive side because it does not use semantic transaction locks.
    administration: RwLock<()>,
}

#[derive(Debug)]
pub(super) struct ConcurrentRuntime {
    identity: DatabaseInstanceIdentity,
    database: ConcurrentDatabase,
}

impl EmbeddedOwner {
    pub(super) fn new(embedded: HawDBEmbedded) -> Self {
        let user_transaction = embedded.transaction_active();
        Self {
            state: Mutex::new(embedded),
            published: Mutex::new(None),
            failed: AtomicBool::new(false),
            user_transaction: AtomicBool::new(user_transaction),
            administration: RwLock::new(()),
        }
    }

    pub(super) fn inspect(&self) -> MutexGuard<'_, HawDBEmbedded> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn autocommit(&self) -> Result<MutexGuard<'_, HawDBEmbedded>> {
        self.ensure_usable()?;
        let state = self.state.lock().map_err(|_| poisoned())?;
        self.ensure_usable()?;
        ensure_autocommit(&state)?;
        Ok(state)
    }

    pub(super) fn execution(&self) -> Result<RwLockReadGuard<'_, ()>> {
        let guard = self.administration.read().map_err(|_| poisoned())?;
        self.ensure_usable()?;
        Ok(guard)
    }

    pub(super) fn administration(&self) -> RwLockWriteGuard<'_, ()> {
        self.administration
            .write()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn direct_write(&self) -> EmbeddedGuard<'_> {
        EmbeddedGuard {
            state: self.inspect(),
            owner: self,
            refresh: false,
            dirty: false,
            retired: None,
        }
    }

    pub(super) fn concurrent(
        self: &Arc<Self>,
        cached: &Mutex<Option<ConcurrentRuntime>>,
    ) -> Result<ConcurrentDatabase> {
        let identity = self.autocommit()?.database().instance_identity()?;
        let mut cached = cached.lock().map_err(|_| poisoned())?;
        if let Some(cached) = cached.as_ref()
            && cached.identity == identity
        {
            return Ok(cached.database.clone());
        }
        let database = ConcurrentDatabase::with_shared_owner(self.clone())?;
        *cached = Some(ConcurrentRuntime {
            identity,
            database: database.clone(),
        });
        Ok(database)
    }

    fn ensure_usable(&self) -> Result<()> {
        if self.is_poisoned() {
            Err(poisoned())
        } else if self.user_transaction.load(Ordering::Acquire) {
            Err(HawDBError::Execution(
                "a user transaction is open on this database".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn guard<'a>(
        &'a self,
        state: MutexGuard<'a, HawDBEmbedded>,
    ) -> Result<SharedDatabaseGuard<'a>> {
        self.ensure_usable()?;
        ensure_autocommit(&state)?;
        Ok(Box::new(DatabaseGuard(EmbeddedGuard {
            state,
            owner: self,
            refresh: true,
            dirty: false,
            retired: None,
        })))
    }
}

impl SharedDatabaseOwner for EmbeddedOwner {
    fn lock(&self) -> Result<SharedDatabaseGuard<'_>> {
        self.guard(self.state.lock().map_err(|_| poisoned())?)
    }

    fn try_lock(&self) -> Result<Option<SharedDatabaseGuard<'_>>> {
        match self.state.try_lock() {
            Ok(state) => self.guard(state).map(Some),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => Err(poisoned()),
        }
    }

    fn is_poisoned(&self) -> bool {
        self.failed.load(Ordering::Acquire)
            || self.state.is_poisoned()
            || self.administration.is_poisoned()
    }

    fn read_view(&self) -> Result<Arc<PublishedConcurrentRead>> {
        self.ensure_usable()?;
        if let Some(view) = self.published.lock().map_err(|_| poisoned())?.as_ref() {
            view.snapshot.ensure_usable()?;
            return Ok(Arc::clone(view));
        }
        let state = self.autocommit()?;
        if let Some(view) = self.published.lock().map_err(|_| poisoned())?.as_ref() {
            view.snapshot.ensure_usable()?;
            return Ok(Arc::clone(view));
        }
        // Keep state -> publication lock order, and never retain the publication
        // mutex across runtime admission/recovery callbacks during cold capture.
        let view = Arc::new(PublishedConcurrentRead::capture(state.database())?);
        *self.published.lock().map_err(|_| poisoned())? = Some(Arc::clone(&view));
        Ok(view)
    }
}

pub(super) struct EmbeddedGuard<'a> {
    // Field order releases the canonical mutex before retiring an old source.
    state: MutexGuard<'a, HawDBEmbedded>,
    owner: &'a EmbeddedOwner,
    refresh: bool,
    dirty: bool,
    retired: Option<Arc<PublishedConcurrentRead>>,
}

impl Deref for EmbeddedGuard<'_> {
    type Target = HawDBEmbedded;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl DerefMut for EmbeddedGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        &mut self.state
    }
}

impl Drop for EmbeddedGuard<'_> {
    fn drop(&mut self) {
        if !self.dirty {
            return;
        }
        if std::thread::panicking() {
            self.owner.failed.store(true, Ordering::Release);
            return;
        }
        self.owner
            .user_transaction
            .store(self.state.transaction_active(), Ordering::Release);
        let next = if self.refresh {
            match PublishedConcurrentRead::capture(self.state.database()) {
                Ok(view) => Some(Arc::new(view)),
                Err(_) => {
                    self.owner.failed.store(true, Ordering::Release);
                    return;
                }
            }
        } else {
            // Administrative callbacks can select an unopened branch. Invalidate
            // lazily instead of opening its runtime just to release this guard.
            None
        };
        let mut published = self
            .owner
            .published
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.retired = std::mem::replace(&mut *published, next);
    }
}

struct DatabaseGuard<'a>(EmbeddedGuard<'a>);
impl Deref for DatabaseGuard<'_> {
    type Target = Database;
    fn deref(&self) -> &Self::Target {
        self.0.database()
    }
}
impl DerefMut for DatabaseGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.database_mut()
    }
}

fn poisoned() -> HawDBError {
    HawDBError::StorageIntegrity("Tokio database publication is poisoned; close and reopen".into())
}

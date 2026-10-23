//! Shared ownership of a canonical database and its host-owned projections.

#[cfg(any(test, feature = "qualification"))]
mod metrics;
#[cfg(feature = "qualification")]
pub use metrics::PublicationLockDurationSnapshot;
#[cfg(any(test, feature = "qualification"))]
pub use metrics::PublicationLockMetricsSnapshot;
#[cfg(any(test, feature = "qualification"))]
use metrics::{LockTiming, PublicationLockMetrics};
#[cfg(any(test, feature = "qualification"))]
use std::time::Instant;

use super::{Database, DatabaseReadTransaction};
use crate::{HawDBError, Result};
use std::ops::{Deref, DerefMut};
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard, TryLockError};

/// State whose graph mutations and projection publications share one owner.
/// The canonical database must remain the same instance throughout a validated
/// publication. Application-specific binding and receipt predicates belong in
/// the validation callback, rather than in this interface.
pub trait DatabasePublicationState {
    fn database(&self) -> &Database;
}

/// Owns the database and projections under a shared publication boundary.
///
/// All mutations of the owned state go through this owner. A validated
/// publication holds exclusive ownership from validation through the complete
/// callback, including any projection checkpoint. Validation receives a pinned
/// canonical read transaction; rejection prevents the publication callback.
///
/// Callbacks must keep reads and work bounded and must not reenter this owner.
/// This lock coordinates publication with graph mutations; it does not add a
/// cross-store rollback or durability protocol. A failed publication retains
/// the underlying operation's existing retry and recovery semantics.
#[derive(Debug)]
pub struct DatabasePublicationOwner<S> {
    state: RwLock<S>,
    #[cfg(any(test, feature = "qualification"))]
    metrics: PublicationLockMetrics,
}

impl<S> DatabasePublicationOwner<S> {
    pub fn new(state: S) -> Self {
        Self {
            state: RwLock::new(state),
            #[cfg(any(test, feature = "qualification"))]
            metrics: PublicationLockMetrics::default(),
        }
    }

    pub fn with_read<T>(&self, operation: impl FnOnce(&S) -> Result<T>) -> Result<T> {
        let state = self.read()?;
        operation(&state)
    }

    pub fn with_write<T>(&self, operation: impl FnOnce(&mut S) -> Result<T>) -> Result<T> {
        let mut state = self.write()?;
        operation(&mut state)
    }

    /// Qualification-only observations of this owner, without acquiring its lock.
    #[cfg(any(test, feature = "qualification"))]
    pub fn lock_metrics(&self) -> PublicationLockMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub(crate) fn read(&self) -> Result<DatabasePublicationReadGuard<'_, S>> {
        #[cfg(any(test, feature = "qualification"))]
        let started = Instant::now();
        let state = self.state.read().map_err(|_| poisoned())?;
        Ok(DatabasePublicationReadGuard {
            state,
            #[cfg(any(test, feature = "qualification"))]
            timing: LockTiming::acquired(&self.metrics.read, started),
        })
    }

    pub(crate) fn write(&self) -> Result<DatabasePublicationWriteGuard<'_, S>> {
        #[cfg(any(test, feature = "qualification"))]
        let started = Instant::now();
        let state = self.state.write().map_err(|_| poisoned())?;
        Ok(DatabasePublicationWriteGuard {
            state,
            #[cfg(any(test, feature = "qualification"))]
            timing: Some(LockTiming::acquired(&self.metrics.write, started)),
        })
    }

    pub(crate) fn try_write(
        &self,
    ) -> std::sync::TryLockResult<DatabasePublicationWriteGuard<'_, S>> {
        #[cfg(any(test, feature = "qualification"))]
        let started = Instant::now();
        match self.state.try_write() {
            Ok(state) => Ok(DatabasePublicationWriteGuard {
                state,
                #[cfg(any(test, feature = "qualification"))]
                timing: Some(LockTiming::acquired(&self.metrics.write, started)),
            }),
            Err(TryLockError::WouldBlock) => {
                #[cfg(any(test, feature = "qualification"))]
                self.metrics.busy_try_write();
                Err(TryLockError::WouldBlock)
            }
            Err(TryLockError::Poisoned(error)) => Err(TryLockError::Poisoned(PoisonError::new(
                DatabasePublicationWriteGuard {
                    state: error.into_inner(),
                    #[cfg(any(test, feature = "qualification"))]
                    timing: None,
                },
            ))),
        }
    }

    pub(crate) fn is_poisoned(&self) -> bool {
        self.state.is_poisoned()
    }
}

impl<S: DatabasePublicationState> DatabasePublicationOwner<S> {
    pub fn with_validated_publication<T>(
        &self,
        validate: impl FnOnce(&mut DatabaseReadTransaction) -> Result<()>,
        publish: impl FnOnce(&mut S) -> Result<T>,
    ) -> Result<T> {
        let mut state = self.write()?;
        state.validate(validate)?;
        publish(&mut state)
    }
}

// Fields drop in declaration order: unlock before recording histogram samples.
pub(crate) struct DatabasePublicationReadGuard<'a, S> {
    state: RwLockReadGuard<'a, S>,
    #[cfg(any(test, feature = "qualification"))]
    timing: LockTiming<'a>,
}

impl<S> Deref for DatabasePublicationReadGuard<'_, S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.state
    }
}

#[cfg(any(test, feature = "qualification"))]
impl<S> Drop for DatabasePublicationReadGuard<'_, S> {
    fn drop(&mut self) {
        self.timing.stop();
    }
}

pub(crate) struct DatabasePublicationWriteGuard<'a, S> {
    state: RwLockWriteGuard<'a, S>,
    #[cfg(any(test, feature = "qualification"))]
    timing: Option<LockTiming<'a>>,
}

#[cfg(any(test, feature = "qualification"))]
impl<S> Drop for DatabasePublicationWriteGuard<'_, S> {
    fn drop(&mut self) {
        if let Some(timing) = &mut self.timing {
            timing.stop();
        }
    }
}

impl<S: DatabasePublicationState> DatabasePublicationWriteGuard<'_, S> {
    pub(crate) fn validate(
        &self,
        validate: impl FnOnce(&mut DatabaseReadTransaction) -> Result<()>,
    ) -> Result<()> {
        let mut snapshot = self.state.database().begin_read_transaction()?;
        validate(&mut snapshot)
    }
}

impl<S> Deref for DatabasePublicationWriteGuard<'_, S> {
    type Target = S;

    fn deref(&self) -> &S {
        &self.state
    }
}

impl<S> DerefMut for DatabasePublicationWriteGuard<'_, S> {
    fn deref_mut(&mut self) -> &mut S {
        &mut self.state
    }
}

fn poisoned() -> HawDBError {
    HawDBError::Execution("database publication owner lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Value;
    use std::sync::Arc;

    struct State {
        graph: Database,
        projection: usize,
    }

    impl DatabasePublicationState for State {
        fn database(&self) -> &Database {
            &self.graph
        }
    }

    #[test]
    fn lock_metrics_count_completed_guards_and_exclude_busy_attempts() {
        let owner = DatabasePublicationOwner::new(0);
        let read = owner.read().unwrap();
        assert_eq!(
            owner.lock_metrics().read_hold.buckets.iter().sum::<u64>(),
            0
        );
        assert!(matches!(owner.try_write(), Err(TryLockError::WouldBlock)));
        drop(read);
        let write = owner.try_write().unwrap();
        assert_eq!(
            owner.lock_metrics().write_hold.buckets.iter().sum::<u64>(),
            0
        );
        drop(write);
        let metrics = owner.lock_metrics();
        for duration in [
            metrics.read_wait,
            metrics.read_hold,
            metrics.write_wait,
            metrics.write_hold,
        ] {
            assert_eq!(duration.buckets.iter().sum::<u64>(), 1);
            assert!(duration.total_nanos >= duration.max_nanos);
        }
        assert_eq!(metrics.busy_try_writes, 1);
    }

    #[test]
    fn lock_metrics_record_unwind_but_not_poisoned_acquisition() {
        let owner = DatabasePublicationOwner::new(0);
        let failed = std::panic::catch_unwind(|| {
            let _write = owner.write().unwrap();
            panic!("qualification unwind");
        });
        assert!(failed.is_err());
        assert!(owner.read().is_err());
        assert!(owner.write().is_err());
        assert!(matches!(owner.try_write(), Err(TryLockError::Poisoned(_))));
        let metrics = owner.lock_metrics();
        assert_eq!(metrics.read_hold.buckets.iter().sum::<u64>(), 0);
        assert_eq!(metrics.write_hold.buckets.iter().sum::<u64>(), 1);
        assert_eq!(metrics.busy_try_writes, 0);
    }

    fn owner() -> Arc<DatabasePublicationOwner<State>> {
        let mut graph = Database::new();
        graph
            .query("CREATE (:Binding {incarnation: 'current'})")
            .unwrap();
        Arc::new(DatabasePublicationOwner::new(State {
            graph,
            projection: 0,
        }))
    }

    fn validate(snapshot: &mut DatabaseReadTransaction) -> Result<()> {
        let rows = snapshot
            .query("MATCH (b:Binding) RETURN b.incarnation AS incarnation LIMIT 2")?
            .rows;
        if rows.len() == 1 && rows[0].get("incarnation") == Some(&Value::String("current".into())) {
            Ok(())
        } else {
            Err(HawDBError::Execution("binding changed".into()))
        }
    }

    #[test]
    fn validation_rejection_cannot_enter_publication() {
        let owner = owner();
        owner
            .with_write(|state| {
                state
                    .graph
                    .query("MATCH (b:Binding) SET b.incarnation = 'stale'")?;
                Ok(())
            })
            .unwrap();
        let mut published = false;
        let error = owner
            .with_validated_publication(validate, |_| {
                published = true;
                Ok(())
            })
            .unwrap_err();
        assert!(error.to_string().contains("binding changed"));
        assert!(!published);
        assert_eq!(owner.with_read(|state| Ok(state.projection)).unwrap(), 0);
    }

    #[test]
    fn validation_and_publication_exclude_other_mutation_owners() {
        let owner = owner();
        owner
            .with_validated_publication(
                |snapshot| {
                    validate(snapshot)?;
                    let other = Arc::clone(&owner);
                    std::thread::spawn(move || assert!(other.try_write().is_err()))
                        .join()
                        .unwrap();
                    Ok(())
                },
                |state| {
                    let other = Arc::clone(&owner);
                    std::thread::spawn(move || assert!(other.try_write().is_err()))
                        .join()
                        .unwrap();
                    state.projection += 1;
                    Ok(())
                },
            )
            .unwrap();
        owner
            .with_write(|state| {
                state
                    .graph
                    .query("MATCH (b:Binding) SET b.incarnation = 'next'")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(owner.with_read(|state| Ok(state.projection)).unwrap(), 1);
        assert!(owner
            .with_validated_publication(validate, |_| Ok(()))
            .is_err());
    }

    #[test]
    fn publication_error_releases_ownership_for_retry() {
        let owner = owner();
        let error = owner
            .with_validated_publication(validate, |_| {
                Err::<(), _>(HawDBError::Execution("publication refused".into()))
            })
            .unwrap_err();
        assert!(error.to_string().contains("publication refused"));
        owner
            .with_validated_publication(validate, |state| {
                state.projection += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(owner.with_read(|state| Ok(state.projection)).unwrap(), 1);
    }
}

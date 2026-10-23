//! Shared ownership of a canonical database and its host-owned projections.

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
}

impl<S> DatabasePublicationOwner<S> {
    pub fn new(state: S) -> Self {
        Self {
            state: RwLock::new(state),
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

    pub(crate) fn read(&self) -> Result<RwLockReadGuard<'_, S>> {
        self.state.read().map_err(|_| poisoned())
    }

    pub(crate) fn write(&self) -> Result<DatabasePublicationWriteGuard<'_, S>> {
        Ok(DatabasePublicationWriteGuard {
            state: self.state.write().map_err(|_| poisoned())?,
        })
    }

    pub(crate) fn try_write(
        &self,
    ) -> std::sync::TryLockResult<DatabasePublicationWriteGuard<'_, S>> {
        match self.state.try_write() {
            Ok(state) => Ok(DatabasePublicationWriteGuard { state }),
            Err(TryLockError::WouldBlock) => Err(TryLockError::WouldBlock),
            Err(TryLockError::Poisoned(error)) => Err(TryLockError::Poisoned(PoisonError::new(
                DatabasePublicationWriteGuard {
                    state: error.into_inner(),
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

pub(crate) struct DatabasePublicationWriteGuard<'a, S> {
    state: RwLockWriteGuard<'a, S>,
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

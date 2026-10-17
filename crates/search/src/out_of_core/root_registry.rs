// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Private native custody, separate from publication exclusion and artifact pins.
//! A cooperating producer/GC protocol is still required before serving activation.

use crate::build_memory::{
    read::{ReadAdmission, ReaderMemory},
    shared::Shared,
};
use crate::error::{HawDBError, Result};
use hawdb_storage::file_descriptors::FileOpenContext;
use hawdb_storage::file_io::{
    File, SourceDirectory, SourceDirectoryIdentity, SourceDirectoryLockMode,
};
use std::collections::LinkedList;
use std::mem::size_of;
use std::path::Path;
use std::sync::RwLock;

mod gate;

const PROCESS_LOCK: &str = ".search-source-process.lock";
static ROOTS: gate::Gate<LinkedList<Entry>> = gate::Gate::new(LinkedList::new());

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Initializing,
    Ready,
    Retiring,
}

struct Entry {
    state: Shared<State>,
    phase: Phase,
    // pop_front frees the node before this receipt is released outside the gate.
    _memory: ReaderMemory,
}

struct NativeResources {
    // Closing the process lock must precede dropping its captured directory.
    process_lock: Option<File>,
    directory: SourceDirectory,
}

struct State {
    identity: SourceDirectoryIdentity,
    native: RwLock<Option<NativeResources>>,
    // SharedAllocation frees its control block before this payload receipt.
    _memory: ReaderMemory,
}

pub(super) struct RootRef(Option<Shared<State>>);

impl RootRef {
    /// Open relative to the retained directory, then return the opening wave
    /// before admitting the held-file read. A pathname replacement cannot
    /// redirect the selected head, and a one-slot operation cannot self-wait.
    pub(super) fn read_head<'execution>(
        &self,
        context: &FileOpenContext,
        name: &str,
        admission: &ReadAdmission<'execution>,
        maximum: u64,
    ) -> Result<crate::build_memory::read::ReadBytes<'execution>> {
        let file = {
            let _io = admission
                .io()
                .map_err(|error| HawDBError::Execution(error.to_string()))?;
            let native = self
                .state()
                .native
                .read()
                .unwrap_or_else(|p| p.into_inner());
            native
                .as_ref()
                .expect("live root owns its directory")
                .directory
                .open_child(context, name)?
        };
        admission.read_unbound_file(&file, maximum)
    }

    pub(super) fn acquire(
        root: &Path,
        context: &FileOpenContext,
        admission: &ReadAdmission<'_>,
    ) -> Result<Self> {
        let state_memory = admission.retained(size_of::<State>() + 2 * size_of::<usize>())?;
        let entry_memory = admission.retained(size_of::<Entry>() + 2 * size_of::<usize>())?;
        let directory = {
            let _io = admission
                .io()
                .map_err(|e| HawDBError::Execution(e.to_string()))?;
            SourceDirectory::capture(root, context)?
        };
        let identity = directory.identity();
        let state = Shared::new(State {
            identity,
            native: RwLock::new(Some(NativeResources {
                process_lock: None,
                directory,
            })),
            _memory: state_memory,
        });
        let existing = {
            let mut roots = registry();
            match roots.iter().find(|entry| entry.state.identity == identity) {
                Some(entry) if entry.phase == Phase::Ready => {
                    // Ready resources have no writer: retirement first changes
                    // phase under this same registry gate. This read performs
                    // only a pointer comparison, never native I/O.
                    let native = entry.state.native.read().unwrap_or_else(|p| p.into_inner());
                    if !native
                        .as_ref()
                        .expect("ready root owns resources")
                        .directory
                        .shares_project_context(context)
                    {
                        return Err(HawDBError::Storage(
                            "source root belongs to another project descriptor domain".into(),
                        ));
                    }
                    Some(entry.state.clone())
                }
                Some(_) => {
                    return Err(HawDBError::Storage(
                        "source root is initializing or retiring".into(),
                    ))
                }
                None => {
                    roots.push_back(Entry {
                        state: state.clone(),
                        phase: Phase::Initializing,
                        _memory: entry_memory,
                    });
                    None
                }
            }
        };
        if let Some(existing) = existing {
            // Candidate directory and unused receipts close outside the gate.
            return Ok(Self(Some(existing)));
        }
        // This guard also retires a failed/cancelled/unwinding initialization.
        let owner = Self(Some(state));
        #[cfg(test)]
        tests::hook(&tests::AFTER_INITIALIZING);
        {
            let _io = admission
                .io()
                .map_err(|e| HawDBError::Execution(e.to_string()))?;
            let mut native = owner
                .state()
                .native
                .write()
                .unwrap_or_else(|p| p.into_inner());
            let native = native
                .as_mut()
                .expect("initializing root owns native resources");
            let lock = native.directory.open_lock_file(
                context,
                PROCESS_LOCK,
                SourceDirectoryLockMode::ReadWriteCreate,
            )?;
            native.process_lock = Some(lock);
            native
                .process_lock
                .as_ref()
                .unwrap()
                .try_lock()
                .map_err(|error| match error {
                    std::fs::TryLockError::WouldBlock => {
                        HawDBError::Storage("source root is owned by another process".into())
                    }
                    std::fs::TryLockError::Error(error) => error.into(),
                })?;
        }
        admission
            .checkpoint()
            .map_err(|e| HawDBError::Execution(e.to_string()))?;
        {
            let mut roots = registry();
            let entry = roots
                .iter_mut()
                .find(|entry| entry.state.identity == identity)
                .expect("initializing entry remains registered");
            entry.phase = Phase::Ready;
        }
        Ok(owner)
    }

    fn state(&self) -> &Shared<State> {
        self.0.as_ref().expect("live root reference")
    }
}

impl Clone for RootRef {
    fn clone(&self) -> Self {
        // A live external reference prevents retirement; the gate coordinates
        // this increment with nonfinal decrements and the final count check.
        let _roots = registry();
        Self(Some(self.state().clone()))
    }
}

impl Drop for RootRef {
    fn drop(&mut self) {
        let state = self.0.take().expect("root reference dropped once");
        {
            let mut roots = registry();
            if state.strong_count() > 2 {
                // Registry + another external owner guarantee no payload dies
                // here. Moving this decrement off-gate leaks on concurrent drop.
                drop(state);
                return;
            }
            assert_eq!(state.strong_count(), 2);
            let entry = roots
                .iter_mut()
                .find(|entry| entry.state.identity == state.identity)
                .expect("live root remains registered");
            entry.phase = Phase::Retiring;
        }
        #[cfg(test)]
        tests::hook(&tests::AFTER_RETIRING);
        let native = state
            .native
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        drop(native);
        #[cfg(test)]
        tests::hook(&tests::AFTER_NATIVE_CLOSE);
        let removed = {
            let mut roots = registry();
            let position = roots
                .iter()
                .position(|entry| entry.state.identity == state.identity)
                .expect("retiring root remains registered");
            let mut tail = roots.split_off(position);
            let removed = tail.pop_front();
            roots.append(&mut tail);
            removed
        };
        // No recursive destruction, native close or final Shared drop under gate.
        drop(removed);
        drop(state);
    }
}

fn registry() -> gate::Guard<'static, LinkedList<Entry>> {
    gate::lock(&ROOTS)
}

#[cfg(test)]
mod tests;

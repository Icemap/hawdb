// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Qualification-only counters. No allocator or runtime control plane.
//! Prepare and pull are disjoint. Nested calls in one phase count once.
//! Snapshots are cumulative; qualification uses a fresh, serial consumer.

use std::cell::Cell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

static CALLS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static NANOS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];
static OVERFLOWED: AtomicBool = AtomicBool::new(false);
thread_local! {
    static DEPTH: Cell<[u32; 2]> = const { Cell::new([0; 2]) };
}

#[derive(Clone, Copy, Debug)]
pub enum Phase {
    Prepare,
    Pull,
}
impl Phase {
    fn index(self) -> usize {
        match self {
            Self::Prepare => 0,
            Self::Pull => 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Snapshot {
    pub prepare_calls: u64,
    pub prepare_ns: u64,
    pub pull_calls: u64,
    pub pull_ns: u64,
    pub overflowed: bool,
}

pub fn snapshot() -> Snapshot {
    Snapshot {
        prepare_calls: CALLS[0].load(Ordering::Relaxed),
        prepare_ns: NANOS[0].load(Ordering::Relaxed),
        pull_calls: CALLS[1].load(Ordering::Relaxed),
        pull_ns: NANOS[1].load(Ordering::Relaxed),
        overflowed: OVERFLOWED.load(Ordering::Relaxed),
    }
}

fn add(counter: &AtomicU64, value: u64) {
    if counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |previous| {
            previous.checked_add(value)
        })
        .is_err()
    {
        OVERFLOWED.store(true, Ordering::Relaxed);
    }
}

pub struct Timer {
    phase: Phase,
    started: Option<Instant>,
    // A nested depth guard must be dropped on the thread that entered it.
    _thread: PhantomData<Rc<()>>,
}
impl Timer {
    pub fn start(phase: Phase) -> Self {
        let outermost = DEPTH.with(|depth| {
            let mut values = depth.get();
            let index = phase.index();
            let outermost = values[index] == 0;
            values[index] = values[index]
                .checked_add(1)
                .expect("qualification phase recursion depth");
            depth.set(values);
            outermost
        });
        Self {
            phase,
            started: outermost.then(Instant::now),
            _thread: PhantomData,
        }
    }
}
impl Drop for Timer {
    fn drop(&mut self) {
        DEPTH.with(|depth| {
            let mut values = depth.get();
            values[self.phase.index()] -= 1;
            depth.set(values);
        });
        if let Some(started) = self.started {
            let Ok(elapsed) = u64::try_from(started.elapsed().as_nanos()) else {
                OVERFLOWED.store(true, Ordering::Relaxed);
                return;
            };
            add(&NANOS[self.phase.index()], elapsed);
            add(&CALLS[self.phase.index()], 1);
        }
    }
}

#[cfg(test)]
mod qualification_observer_checks {
    use super::*;
    #[test]
    fn nested_entrypoints_unwind_and_overflow_preserve_measurement_validity() {
        let initial = snapshot();
        {
            let _outer = Timer::start(Phase::Prepare);
            let _inner = Timer::start(Phase::Prepare);
            assert_eq!(snapshot().prepare_calls, initial.prepare_calls);
        }
        let prepared = snapshot();
        assert_eq!(prepared.prepare_calls, initial.prepare_calls + 1);
        assert_eq!(prepared.pull_calls, initial.pull_calls);
        let result = std::panic::catch_unwind(|| {
            let _pull = Timer::start(Phase::Pull);
            panic!("simulated consumer failure");
        });
        assert!(result.is_err());
        { let _retry = Timer::start(Phase::Pull); }
        let retried = snapshot();
        assert_eq!(retried.pull_calls, initial.pull_calls + 2);
        assert!(!retried.overflowed);
        let full = AtomicU64::new(u64::MAX);
        add(&full, 1);
        assert_eq!(full.load(Ordering::Relaxed), u64::MAX);
        assert!(snapshot().overflowed);
    }
}

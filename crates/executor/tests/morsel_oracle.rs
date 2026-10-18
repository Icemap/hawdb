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

use hawdb_core::error::FileDescriptorError;
use hawdb_core::{
    HawDBError, ReadBudgetResource, Result, RuntimeCancellationReason, RuntimeCancellationToken,
    RuntimeTaskContext,
};
use hawdb_executor::{
    execute_morsels_ordered, BoundedExecutor, Morsel, MorselAdmission, MorselAdmissionRequest,
    MorselOrdinal, MorselOutput, MorselStreamControl, MorselStreamResources, PipelineId,
    QueryMemoryClass, QueryMemoryLedger, SequentialMorselScheduler, SharedExecutorPool,
    SharedPoolMorselScheduler,
};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

fn nz(value: usize) -> NonZeroUsize {
    NonZeroUsize::new(value).unwrap()
}

struct Fixture {
    input: Vec<u64>,
    target_rows: usize,
    requested_workers: usize,
    memory_workers: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct RowChunk {
    morsel: Morsel,
    values: Vec<u64>,
}

fn transform(value: u64, row: usize) -> u64 {
    value
        .wrapping_mul(0x9e37_79b9)
        .rotate_left((row % 64) as u32)
}

impl Fixture {
    fn admission(&self) -> MorselAdmission {
        let admission = MorselAdmission::try_new(MorselAdmissionRequest {
            pipeline_id: PipelineId(227),
            input_rows: self.input.len(),
            target_rows: nz(self.target_rows),
            requested_parallelism: nz(self.requested_workers),
            bytes_per_worker: nz(64),
            memory_budget_bytes: nz(self.memory_workers * 64),
        })
        .unwrap();
        let expected_count = self.input.chunks(self.target_rows).count();
        let workers = expected_count
            .min(self.requested_workers)
            .min(self.memory_workers);
        assert_eq!(admission.morsel_count(), expected_count);
        assert_eq!(admission.max_workers(), workers);
        assert_eq!(admission.reserved_bytes(), workers * 64);
        admission
    }

    fn expected(&self) -> Vec<RowChunk> {
        // Neither partition boundaries nor values come from the scheduler's iterator.
        let transformed = self
            .input
            .iter()
            .enumerate()
            .map(|(row, &value)| transform(value, row))
            .collect::<Vec<_>>();
        transformed
            .chunks(self.target_rows)
            .enumerate()
            .map(|(index, values)| RowChunk {
                morsel: Morsel {
                    pipeline_id: PipelineId(227),
                    ordinal: MorselOrdinal(index as u64),
                    start_row: index * self.target_rows,
                    row_count: values.len(),
                },
                values: values.to_vec(),
            })
            .collect()
    }

    fn work(
        &self,
        morsel: Morsel,
        calls: &[AtomicUsize],
        gate: Option<&CompletionGate>,
    ) -> RowChunk {
        let index = morsel.ordinal.0 as usize;
        assert_eq!(
            calls[index].fetch_add(1, Ordering::SeqCst),
            0,
            "morsel executed twice: {index}"
        );
        let values = self.input[morsel.start_row..morsel.start_row + morsel.row_count]
            .iter()
            .enumerate()
            .map(|(offset, &value)| transform(value, morsel.start_row + offset))
            .collect();
        if let Some(gate) = gate {
            gate.complete(index);
        } else if index.is_multiple_of(3) {
            std::thread::yield_now();
        }
        RowChunk { morsel, values }
    }
}

// Bounded coordination makes later workers finish before ordinal zero. A lost
// worker fails the test instead of leaving an unbounded barrier/spin behind.
struct CompletionGate {
    participants: usize,
    sender: mpsc::Sender<()>,
    receiver: Mutex<mpsc::Receiver<()>>,
    completed: Mutex<Vec<usize>>,
}

impl CompletionGate {
    fn new(participants: usize) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            participants,
            sender,
            receiver: Mutex::new(receiver),
            completed: Mutex::new(Vec::new()),
        }
    }

    fn complete(&self, index: usize) {
        if index == 0 {
            for _ in 1..self.participants {
                self.receiver
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .expect("later worker did not complete");
            }
        }
        self.completed.lock().unwrap().push(index);
        if index > 0 && index < self.participants {
            self.sender.send(()).unwrap();
        }
    }

    fn assert_reordered(&self) {
        let completed = self.completed.lock().unwrap();
        assert!(completed.iter().position(|&index| index == 0).unwrap() >= self.participants - 1);
    }
}

#[derive(Debug, Clone, Copy)]
enum Materializer {
    Sequential,
    Ordered,
    Map,
    ContextMap,
    Shared,
    ContextShared,
}

const MATERIALIZERS: [Materializer; 6] = [
    Materializer::Sequential,
    Materializer::Ordered,
    Materializer::Map,
    Materializer::ContextMap,
    Materializer::Shared,
    Materializer::ContextShared,
];

#[derive(Default)]
struct WorkerActivity {
    active: AtomicUsize,
    peak: AtomicUsize,
}

impl WorkerActivity {
    fn enter(&self) -> WorkerGuard<'_> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(active, Ordering::SeqCst);
        WorkerGuard(self)
    }
}

struct WorkerGuard<'a>(&'a WorkerActivity);

impl Drop for WorkerGuard<'_> {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

fn assert_materializer(
    fixture: &Fixture,
    pool: &SharedExecutorPool,
    mode: Materializer,
    gate: Option<&CompletionGate>,
) {
    let admission = fixture.admission();
    let morsels = admission.morsels().collect::<Vec<_>>();
    let calls = (0..morsels.len())
        .map(|_| AtomicUsize::new(0))
        .collect::<Vec<_>>();
    let workers = WorkerActivity::default();
    let work = |morsel| {
        let _worker = workers.enter();
        fixture.work(morsel, &calls, gate)
    };
    let executor = || BoundedExecutor::with_pool(nz(admission.max_workers().max(1)), pool.clone());
    let scheduler = SharedPoolMorselScheduler::new(pool.clone());
    let context = RuntimeTaskContext::without_deadline(RuntimeCancellationToken::new());
    let actual = match mode {
        Materializer::Sequential => SequentialMorselScheduler
            .execute(&admission, |m| Ok(work(m)))
            .unwrap(),
        Materializer::Ordered => execute_morsels_ordered(&admission, |m| Ok(work(m))).unwrap(),
        Materializer::Map => executor().map_ordered(&morsels, |&m| work(m)),
        Materializer::ContextMap => executor()
            .map_ordered_with_context(&morsels, &context, |&m| work(m))
            .unwrap(),
        Materializer::Shared => scheduler.execute(&admission, |m| Ok(work(m))).unwrap(),
        Materializer::ContextShared => scheduler
            .execute_with_context(&admission, &context, |m| Ok(work(m)))
            .unwrap(),
    };
    assert_eq!(actual, fixture.expected(), "materializer={mode:?}");
    assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 1));
    assert_eq!(workers.active.load(Ordering::SeqCst), 0);
    let worker_limit = if matches!(mode, Materializer::Sequential | Materializer::Ordered) {
        morsels.len().min(1)
    } else {
        admission.max_workers().min(pool.worker_count())
    };
    assert!(
        workers.peak.load(Ordering::SeqCst) <= worker_limit,
        "materializer={mode:?} exceeded {worker_limit} active workers"
    );
}

#[derive(Debug, Default)]
struct Lifetimes {
    created: AtomicUsize,
    live: AtomicUsize,
    dropped: AtomicUsize,
    peak: AtomicUsize,
}

struct TrackedOutput {
    chunk: RowChunk,
    lifetimes: Arc<Lifetimes>,
}

impl TrackedOutput {
    fn new(chunk: RowChunk, lifetimes: &Arc<Lifetimes>) -> Self {
        lifetimes.created.fetch_add(1, Ordering::SeqCst);
        let live = lifetimes.live.fetch_add(1, Ordering::SeqCst) + 1;
        lifetimes.peak.fetch_max(live, Ordering::SeqCst);
        Self {
            chunk,
            lifetimes: Arc::clone(lifetimes),
        }
    }

    fn resident_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.chunk.values.capacity() * std::mem::size_of::<u64>()
    }
}

impl Drop for TrackedOutput {
    fn drop(&mut self) {
        self.lifetimes.live.fetch_sub(1, Ordering::SeqCst);
        self.lifetimes.dropped.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Exit {
    Complete,
    Stop(usize),
    WorkerError(usize),
    WorkerPanic(usize),
    ConsumerError(usize),
    ConsumerPanic(usize),
    CancelBefore,
    CancelWorker(usize),
    Oversize(usize),
    DenyReservation,
}

fn assert_stream(
    fixture: &Fixture,
    pool: &SharedExecutorPool,
    exit: Exit,
    gate: Option<&CompletionGate>,
) {
    let admission = fixture.admission();
    let expected = fixture.expected();
    let workers = admission.max_workers().min(pool.worker_count());
    let reservation =
        std::mem::size_of::<TrackedOutput>() + fixture.target_rows * std::mem::size_of::<u64>();
    let budget = if exit == Exit::DenyReservation {
        1
    } else {
        workers.max(1) * reservation
    };
    let ledger = QueryMemoryLedger::new(nz(budget));
    let account = ledger.account(
        QueryMemoryClass::MorselOutput,
        "external scheduler oracle",
        nz(budget),
    );
    let lifetimes = Arc::new(Lifetimes::default());
    let calls = (0..expected.len())
        .map(|_| AtomicUsize::new(0))
        .collect::<Vec<_>>();
    let token = RuntimeCancellationToken::new();
    let context = RuntimeTaskContext::without_deadline(token.clone());
    if exit == Exit::CancelBefore {
        token.cancel();
    }
    let mut consumed = Vec::new();
    let result = SharedPoolMorselScheduler::new(pool.clone()).execute_accounted_ordered(
        &admission,
        MorselStreamResources {
            task_context: Some(&context),
            output_account: &account,
            output_reservation_bytes: nz(reservation),
        },
        |morsel| {
            // The reservation must already be live before any output is allocated.
            assert!(ledger.snapshot().used_bytes >= reservation);
            let output = TrackedOutput::new(fixture.work(morsel, &calls, gate), &lifetimes);
            let index = morsel.ordinal.0 as usize;
            if exit == Exit::WorkerError(index) {
                return Err(HawDBError::Execution("injected worker failure".into()));
            }
            assert_ne!(exit, Exit::WorkerPanic(index), "injected worker panic");
            if exit == Exit::CancelWorker(index) {
                token.cancel();
            }
            let bytes = if exit == Exit::Oversize(index) {
                reservation + 1
            } else {
                output.resident_bytes()
            };
            Ok(MorselOutput::new(output, bytes))
        },
        |morsel, output| {
            assert_eq!(morsel, output.chunk.morsel);
            assert!(
                ledger.snapshot().used_bytes >= output.resident_bytes(),
                "output lease ended before consumption"
            );
            let index = morsel.ordinal.0 as usize;
            consumed.push(RowChunk {
                morsel,
                values: output.chunk.values.clone(),
            });
            assert_ne!(exit, Exit::ConsumerPanic(index), "injected consumer panic");
            if exit == Exit::ConsumerError(index) {
                return Err(HawDBError::Execution("injected consumer failure".into()));
            }
            Ok(if exit == Exit::Stop(index) {
                MorselStreamControl::Stop
            } else {
                MorselStreamControl::Continue
            })
        },
    );
    assert_eq!(consumed, expected[..consumed.len()], "exit={exit:?}");
    match exit {
        Exit::Complete | Exit::Stop(_) => {
            let report = result.unwrap();
            let expected_len = match exit {
                Exit::Stop(index) => index + 1,
                _ => expected.len(),
            };
            assert_eq!(consumed.len(), expected_len);
            assert_eq!(report.stopped_early, matches!(exit, Exit::Stop(_)));
            assert!(report.peak_buffered_outputs <= workers);
            assert!(report.peak_reorder_entries <= workers);
            assert!(report.peak_buffered_output_bytes <= workers * reservation);
            if exit == Exit::Complete {
                assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 1));
            }
        }
        _ => {
            let error = result.unwrap_err().to_string();
            match exit {
                Exit::WorkerError(index) => {
                    assert!(error.contains("injected worker failure"), "{error}");
                    assert!(consumed.len() <= index);
                }
                Exit::ConsumerError(index) => {
                    assert!(error.contains("injected consumer failure"), "{error}");
                    assert_eq!(consumed.len(), index + 1);
                }
                Exit::WorkerPanic(index) => {
                    assert!(
                        error.contains(&format!("worker panicked at ordinal {index}")),
                        "{error}"
                    );
                    assert!(consumed.len() <= index);
                }
                Exit::ConsumerPanic(index) => {
                    assert!(
                        error.contains(&format!("consumer panicked at ordinal {index}")),
                        "{error}"
                    );
                    assert_eq!(consumed.len(), index + 1);
                }
                Exit::Oversize(index) => {
                    assert!(
                        error.contains(&format!("output at ordinal {index}"))
                            && error.contains("reservation"),
                        "{error}"
                    );
                    assert!(consumed.len() <= index);
                }
                Exit::DenyReservation => {
                    assert!(error.contains("budget"), "{error}");
                    assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 0));
                }
                Exit::CancelBefore => {
                    assert!(error.contains("cancelled"), "{error}");
                    assert!(calls.iter().all(|count| count.load(Ordering::SeqCst) == 0));
                }
                Exit::CancelWorker(_) => assert!(error.contains("cancelled"), "{error}"),
                Exit::Complete | Exit::Stop(_) => unreachable!(),
            }
        }
    }
    if let Exit::Stop(index) | Exit::ConsumerError(index) | Exit::ConsumerPanic(index) = exit {
        assert!(
            calls
                .iter()
                .skip(index + workers)
                .all(|count| count.load(Ordering::SeqCst) == 0),
            "opened another work window after stop/error"
        );
    }
    assert_eq!(lifetimes.live.load(Ordering::SeqCst), 0, "exit={exit:?}");
    assert_eq!(
        lifetimes.created.load(Ordering::SeqCst),
        lifetimes.dropped.load(Ordering::SeqCst)
    );
    assert!(lifetimes.peak.load(Ordering::SeqCst) <= workers);
    let snapshot = ledger.snapshot();
    assert_eq!(snapshot.used_bytes, 0, "exit={exit:?}");
    assert!(snapshot.peak_bytes <= budget);
    assert!(snapshot.classes.iter().all(|class| class.used_bytes == 0));
}

#[test]
fn scheduler_apis_match_independent_row_oracle() {
    for pool_workers in [1, 2, 4] {
        let pool = SharedExecutorPool::new(nz(pool_workers)).unwrap();
        for rows in [0, 1, 6, 7, 8, 32, 65] {
            for (target_rows, requested_workers, memory_workers) in
                [(1, 4, 3), (7, 4, 2), (16, 1, 4)]
            {
                let fixture = Fixture {
                    input: (0..rows)
                        .map(|row| (row as u64).wrapping_mul(0x5eed))
                        .collect(),
                    target_rows,
                    requested_workers,
                    memory_workers,
                };
                for mode in MATERIALIZERS {
                    assert_materializer(&fixture, &pool, mode, None);
                }
                assert_stream(&fixture, &pool, Exit::Complete, None);
            }
        }
    }
}

#[test]
fn out_of_order_work_keeps_coordinator_order() {
    let fixture = Fixture {
        input: (0..37).collect(),
        target_rows: 4,
        requested_workers: 3,
        memory_workers: 3,
    };
    let pool = SharedExecutorPool::new(nz(4)).unwrap();
    for mode in [
        Materializer::Map,
        Materializer::ContextMap,
        Materializer::Shared,
        Materializer::ContextShared,
    ] {
        let gate = CompletionGate::new(3);
        assert_materializer(&fixture, &pool, mode, Some(&gate));
        gate.assert_reordered();
    }
    let gate = CompletionGate::new(3);
    assert_stream(&fixture, &pool, Exit::Complete, Some(&gate));
    gate.assert_reordered();
}

#[test]
fn accounted_stream_exit_paths_preserve_prefixes_and_release_resources() {
    let fixture = Fixture {
        input: (0..43).collect(),
        target_rows: 4,
        requested_workers: 4,
        memory_workers: 3,
    };
    for pool_workers in [1, 2, 4] {
        let pool = SharedExecutorPool::new(nz(pool_workers)).unwrap();
        for index in [0, 5, 10] {
            for exit in [
                Exit::Stop(index),
                Exit::WorkerError(index),
                Exit::WorkerPanic(index),
                Exit::ConsumerError(index),
                Exit::ConsumerPanic(index),
                Exit::CancelWorker(index),
                Exit::Oversize(index),
            ] {
                assert_stream(&fixture, &pool, exit, None);
            }
        }
        assert_stream(&fixture, &pool, Exit::DenyReservation, None);
        assert_stream(&fixture, &pool, Exit::CancelBefore, None);
    }
}

#[test]
fn contextual_materializers_reject_pre_cancelled_empty_and_nonempty_inputs() {
    let pool = SharedExecutorPool::new(nz(2)).unwrap();
    for rows in [0, 19] {
        let fixture = Fixture {
            input: (0..rows).collect(),
            target_rows: 4,
            requested_workers: 2,
            memory_workers: 2,
        };
        let admission = fixture.admission();
        let token = RuntimeCancellationToken::new();
        let context = RuntimeTaskContext::without_deadline(token.clone());
        token.cancel();
        let calls = AtomicUsize::new(0);
        let error = BoundedExecutor::with_pool(nz(2), pool.clone())
            .map_ordered_with_context(&fixture.input, &context, |_| {
                calls.fetch_add(1, Ordering::SeqCst)
            })
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        let error = SharedPoolMorselScheduler::new(pool.clone())
            .execute_with_context(&admission, &context, |_| {
                Ok(calls.fetch_add(1, Ordering::SeqCst))
            })
            .unwrap_err();
        assert!(error.to_string().contains("cancelled"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_stream(&fixture, &pool, Exit::CancelBefore, None);
    }
}

#[test]
fn materializing_error_order_does_not_imply_parallel_fail_fast() {
    let fixture = Fixture {
        input: (0..25).collect(),
        target_rows: 4,
        requested_workers: 3,
        memory_workers: 3,
    };
    let admission = fixture.admission();
    let pool = SharedExecutorPool::new(nz(3)).unwrap();
    for mode in MATERIALIZERS {
        let calls = AtomicUsize::new(0);
        let work = |morsel: Morsel| -> Result<u64> {
            calls.fetch_add(1, Ordering::SeqCst);
            match morsel.ordinal.0 {
                2 | 4 => Err(HawDBError::Execution(format!(
                    "failure {}",
                    morsel.ordinal.0
                ))),
                ordinal => Ok(ordinal),
            }
        };
        let context = RuntimeTaskContext::without_deadline(RuntimeCancellationToken::new());
        let scheduler = SharedPoolMorselScheduler::new(pool.clone());
        let morsels = admission.morsels().collect::<Vec<_>>();
        let executor = || BoundedExecutor::with_pool(nz(3), pool.clone());
        let result = match mode {
            Materializer::Sequential => SequentialMorselScheduler.execute(&admission, work),
            Materializer::Ordered => execute_morsels_ordered(&admission, work),
            Materializer::Shared => scheduler.execute(&admission, work),
            Materializer::ContextShared => {
                scheduler.execute_with_context(&admission, &context, work)
            }
            Materializer::Map | Materializer::ContextMap => {
                let output = if matches!(mode, Materializer::Map) {
                    executor().map_ordered(&morsels, |&m| work(m))
                } else {
                    executor()
                        .map_ordered_with_context(&morsels, &context, |&m| work(m))
                        .unwrap()
                };
                assert_eq!(
                    output
                        .iter()
                        .enumerate()
                        .filter(|(_, value)| value.is_err())
                        .map(|(index, _)| index)
                        .collect::<Vec<_>>(),
                    [2, 4]
                );
                output.into_iter().collect()
            }
        };
        assert_eq!(
            result.unwrap_err().to_string(),
            "execution error: failure 2"
        );
        let expected_calls = if matches!(mode, Materializer::Sequential | Materializer::Ordered) {
            3
        } else {
            7
        };
        assert_eq!(calls.load(Ordering::SeqCst), expected_calls);
    }
}

#[test]
fn admission_extremes_do_not_wrap_partition_boundaries() {
    let admission = MorselAdmission::try_new(MorselAdmissionRequest {
        pipeline_id: PipelineId(227),
        input_rows: usize::MAX,
        target_rows: nz(usize::MAX - 1),
        requested_parallelism: nz(2),
        bytes_per_worker: nz(1),
        memory_budget_bytes: nz(2),
    })
    .unwrap();
    let morsels = admission.morsels().collect::<Vec<_>>();
    assert_eq!(morsels.len(), 2);
    assert_eq!(
        (morsels[0].start_row, morsels[0].row_count),
        (0, usize::MAX - 1)
    );
    assert_eq!(
        (morsels[1].start_row, morsels[1].row_count),
        (usize::MAX - 1, 1)
    );
    assert_eq!(morsels[1].ordinal, MorselOrdinal(1));
}

#[test]
#[ignore = "local-only deterministic public scheduler differential campaign"]
fn scheduler_differential_campaign() {
    let pools = [1, 2, 4].map(|workers| SharedExecutorPool::new(nz(workers)).unwrap());
    let mut cases = 0;
    for seed in [227_u64, 7, 0x5eed] {
        let mut state = seed;
        for case in 0..192 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let fixture = Fixture {
                input: (0..state as usize % 130)
                    .map(|row| state.rotate_left(row as u32))
                    .collect(),
                target_rows: 1 + state.rotate_left(7) as usize % 17,
                requested_workers: 1 + state.rotate_left(19) as usize % 6,
                memory_workers: 1 + state.rotate_left(29) as usize % 4,
            };
            let pool = &pools[case % pools.len()];
            for mode in MATERIALIZERS {
                assert_materializer(&fixture, pool, mode, None);
            }
            let count = fixture.expected().len();
            let index = state as usize % count.max(1);
            let exit = if count == 0 {
                Exit::Complete
            } else {
                match case % 8 {
                    0 => Exit::Stop(index),
                    1 => Exit::WorkerError(index),
                    2 => Exit::ConsumerError(index),
                    3 => Exit::CancelBefore,
                    4 => Exit::CancelWorker(index),
                    5 => Exit::Oversize(index),
                    6 => Exit::DenyReservation,
                    _ => Exit::Complete,
                }
            };
            assert_stream(&fixture, pool, exit, None);
            cases += 1;
        }
    }
    terminal_worker_resource_error_cancels_live_sibling_and_keeps_original_cause();
    consumer_resource_error_cancels_live_worker_and_releases_all_leases();
    serial_resource_failures_signal_exhaustion_but_ordinary_errors_do_not();
    output_admission_failure_signals_before_running_any_callback();
    recovered_reservation_does_not_cancel_the_execution_group();
    println!("Public scheduler differential: {cases} fixtures, {} materializing comparisons and {cases} accounted-stream lifecycles", cases * MATERIALIZERS.len());
}

fn admission(workers: usize) -> MorselAdmission {
    MorselAdmission::try_new(MorselAdmissionRequest {
        pipeline_id: PipelineId(996),
        input_rows: 8,
        target_rows: nz(1),
        requested_parallelism: nz(workers),
        bytes_per_worker: nz(32),
        memory_budget_bytes: nz(1024),
    })
    .unwrap()
}

fn wait_for_exhaustion(context: &RuntimeTaskContext, observed: &AtomicBool) -> HawDBError {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Err(reason) = context.checkpoint() {
            assert_eq!(reason, RuntimeCancellationReason::ResourceExhausted);
            observed.store(true, Ordering::Release);
            return HawDBError::Execution(format!("sibling stopped: {reason}"));
        }
        assert!(Instant::now() < deadline, "sibling was not cancelled");
        std::thread::yield_now();
    }
}

#[test]
fn terminal_worker_resource_error_cancels_live_sibling_and_keeps_original_cause() {
    for materialize in [false, true] {
        let scheduler = SharedPoolMorselScheduler::new(SharedExecutorPool::new(nz(2)).unwrap());
        let ledger = QueryMemoryLedger::new(nz(1024));
        let output = ledger.account(QueryMemoryClass::MorselOutput, "outputs", nz(64));
        let working = ledger.account(QueryMemoryClass::BlockingState, "worker state", nz(16));
        let parent = RuntimeTaskContext::default();
        let task = parent.child();
        let observed = AtomicBool::new(false);
        let calls = AtomicUsize::new(0);
        let consumed = AtomicUsize::new(0);
        let (started, receiver) = mpsc::sync_channel(1);
        let receiver = Mutex::new(receiver);
        let execute = |morsel: Morsel| -> Result<MorselOutput<u64>> {
            calls.fetch_add(1, Ordering::SeqCst);
            if morsel.ordinal.0 == 0 {
                let _lease = working.reserve(8)?;
                started.send(()).unwrap();
                return Err(wait_for_exhaustion(&task, &observed));
            }
            receiver
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            let _lease = working.reserve(16)?;
            panic!("the account must refuse 8 + 16 > 16");
        };
        let error = if materialize {
            scheduler
                .execute_with_context(&admission(2), &task, execute)
                .unwrap_err()
        } else {
            scheduler
                .execute_accounted_ordered(
                    &admission(2),
                    MorselStreamResources {
                        task_context: Some(&task),
                        output_account: &output,
                        output_reservation_bytes: nz(32),
                    },
                    execute,
                    |_, _| {
                        consumed.fetch_add(1, Ordering::SeqCst);
                        Ok(MorselStreamControl::Continue)
                    },
                )
                .unwrap_err()
        };
        let HawDBError::ResourceBudgetExceeded(error) = error else {
            panic!("resource cause was replaced: {error:?}");
        };
        assert_eq!((error.requested, error.used, error.budget), (16, 8, 16));
        assert_eq!(error.owner, "worker state");
        assert!(observed.load(Ordering::Acquire));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(consumed.load(Ordering::SeqCst), 0);
        assert_eq!(ledger.snapshot().used_bytes, 0);
        assert_eq!(parent.checkpoint(), Ok(()));
    }
}

#[test]
fn consumer_resource_error_cancels_live_worker_and_releases_all_leases() {
    let scheduler = SharedPoolMorselScheduler::new(SharedExecutorPool::new(nz(2)).unwrap());
    let ledger = QueryMemoryLedger::new(nz(1024));
    let output = ledger.account(QueryMemoryClass::MorselOutput, "outputs", nz(64));
    let working = ledger.account(QueryMemoryClass::BlockingState, "consumer state", nz(16));
    let task = RuntimeTaskContext::default();
    let observed = AtomicBool::new(false);
    let (started, receiver) = mpsc::sync_channel(1);
    let receiver = Mutex::new(receiver);
    let error = scheduler
        .execute_accounted_ordered(
            &admission(2),
            MorselStreamResources {
                task_context: Some(&task),
                output_account: &output,
                output_reservation_bytes: nz(32),
            },
            |morsel| {
                if morsel.ordinal.0 == 0 {
                    receiver
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap();
                    return Ok(MorselOutput::new(0_u64, 8));
                }
                let _lease = working.reserve(8)?;
                started.send(()).unwrap();
                Err(wait_for_exhaustion(&task, &observed))
            },
            |_, _| {
                let _lease = working.reserve(16)?;
                panic!("the consumer must fail admission");
            },
        )
        .unwrap_err();
    assert!(
        matches!(error, HawDBError::ResourceBudgetExceeded(ref error) if error.owner == "consumer state")
    );
    assert!(observed.load(Ordering::Acquire));
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn serial_resource_failures_signal_exhaustion_but_ordinary_errors_do_not() {
    let scheduler = SharedPoolMorselScheduler::new(SharedExecutorPool::new(nz(1)).unwrap());
    let ledger = QueryMemoryLedger::new(nz(1024));
    let output = ledger.account(QueryMemoryClass::MorselOutput, "outputs", nz(32));
    for (error, resource) in [
        (output.reserve(33).unwrap_err(), true),
        (
            HawDBError::read_budget_exceeded(ReadBudgetResource::Rows, 1, "row budget"),
            true,
        ),
        (
            HawDBError::GraphExpansionCandidateLimitExceeded {
                requested: 2,
                limit: 1,
            },
            true,
        ),
        (
            HawDBError::GraphExpansionPayloadLimitExceeded {
                requested: 2,
                limit: 1,
            },
            true,
        ),
        (
            HawDBError::FileDescriptors(FileDescriptorError::BudgetExceeded {
                requested: 2,
                available: 1,
                limit: 1,
            }),
            true,
        ),
        (
            HawDBError::FileDescriptors(FileDescriptorError::OsLimit {
                requested: 1,
                os_code: None,
                soft: None,
                hard: None,
            }),
            true,
        ),
        (
            HawDBError::FileDescriptors(FileDescriptorError::InvalidBudget { limit: 0 }),
            false,
        ),
        (
            HawDBError::FileDescriptors(FileDescriptorError::ConfigurationConflict {
                configured: 2,
                requested: 1,
            }),
            false,
        ),
        (HawDBError::Execution("budget exhausted".into()), false),
    ] {
        for consumer in [false, true] {
            let task = RuntimeTaskContext::default();
            let result = scheduler.execute_accounted_ordered(
                &admission(1),
                MorselStreamResources {
                    task_context: Some(&task),
                    output_account: &output,
                    output_reservation_bytes: nz(32),
                },
                |_| {
                    if consumer {
                        Ok(MorselOutput::new((), 0))
                    } else {
                        Err(error.clone())
                    }
                },
                |_, _| Err(error.clone()),
            );
            assert_eq!(result.unwrap_err(), error);
            assert_eq!(
                task.checkpoint(),
                if resource {
                    Err(RuntimeCancellationReason::ResourceExhausted)
                } else {
                    Ok(())
                }
            );
            assert_eq!(ledger.snapshot().used_bytes, 0);
        }
    }
}

#[test]
fn output_admission_failure_signals_before_running_any_callback() {
    let scheduler = SharedPoolMorselScheduler::new(SharedExecutorPool::new(nz(2)).unwrap());
    let ledger = QueryMemoryLedger::new(nz(1024));
    let output = ledger.account(QueryMemoryClass::MorselOutput, "outputs", nz(16));
    let task = RuntimeTaskContext::default();
    let error = scheduler
        .execute_accounted_ordered(
            &admission(2),
            MorselStreamResources {
                task_context: Some(&task),
                output_account: &output,
                output_reservation_bytes: nz(32),
            },
            |_| -> Result<MorselOutput<()>> { panic!("output must be admitted first") },
            |_, _| panic!("no output is admitted"),
        )
        .unwrap_err();
    assert!(matches!(error, HawDBError::ResourceBudgetExceeded(_)));
    assert_eq!(
        task.checkpoint(),
        Err(RuntimeCancellationReason::ResourceExhausted)
    );
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

#[test]
fn recovered_reservation_does_not_cancel_the_execution_group() {
    let scheduler = SharedPoolMorselScheduler::new(SharedExecutorPool::new(nz(2)).unwrap());
    let ledger = QueryMemoryLedger::new(nz(1024));
    let output = ledger.account(QueryMemoryClass::MorselOutput, "outputs", nz(64));
    let working = ledger.account(QueryMemoryClass::BlockingState, "recoverable", nz(16));
    let task = RuntimeTaskContext::default();
    let mut received = Vec::new();
    scheduler
        .execute_accounted_ordered(
            &admission(2),
            MorselStreamResources {
                task_context: Some(&task),
                output_account: &output,
                output_reservation_bytes: nz(32),
            },
            |morsel| {
                assert!(matches!(
                    working.reserve(17),
                    Err(HawDBError::ResourceBudgetExceeded(_))
                ));
                assert_eq!(task.checkpoint(), Ok(()));
                Ok(MorselOutput::new(morsel.ordinal.0, 8))
            },
            |_, value| {
                received.push(value);
                Ok(MorselStreamControl::Continue)
            },
        )
        .unwrap();
    assert_eq!(received, (0..8).collect::<Vec<_>>());
    assert_eq!(task.checkpoint(), Ok(()));
    assert_eq!(ledger.snapshot().used_bytes, 0);
}

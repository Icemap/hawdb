# Prepared query admission ownership

`HawDBPreparedAdmission.tla` models two queries sharing one CPU slot and a
bounded memory governor. Preparation owns a temporary reader pin and a fixed
memory reservation. Transfer releases CPU/task/I/O ownership and the reader
pin while retaining the memory charge. Execution separately admits its working
and result memory; normal completion, failure, cancellation and panic retire
all ownership. On any admission refusal, rebuildable preparation is
released before retrying. A hard refusal enters `retrying` without retaining
memory, CPU or a source pin. The next admission tests execution alone: it can
succeed in the band `execution <= capacity < execution + preparation`, queue
for transient capacity, or terminate when execution itself cannot fit.
The execution callback rebuilds preparation under its new
reservation. Queued queries therefore cannot retain each other's required
execution capacity; immediate admissions still reuse preparation.

The model checks that prepared state remains charged, a waiting query retains
neither CPU nor a source pin, terminal paths release ownership, and new
admission cannot overcommit capacity. A request's retained preparation plus its
new reservation is checked against hard capacity before transient saturation.
This rules out an impossible self-wait. Capacity can change after preparation;
on retry, an over-capacity waiter can terminate. A shrink does not revoke work
already admitted. The capacity represents the minimum of the governor and
shared process-policy ceilings, not transient RSS headroom.

Four negative controls remove the transfer charge, retain the only CPU slot,
omit the retained-memory floor, and keep prepared memory while queued. Each must violate its named invariant.
Two additional reachability controls require counterexamples witnessing both
successful execution after a hard refusal and a final hard refusal after the
preparation has been released. They prevent the model from silently removing
either fallback outcome. `retrying` is an immediate admission attempt, not a
queued wait, so the waiting-capacity invariant applies only after that attempt.
Tokio queue-ticket renewal after a terminal refusal is owned by the native
capacity-band regression; this model does not represent queue identities.
These safety checks do not prove fairness under indefinite external resource
pressure, allocator-accurate plan sizing, or query-plan validity. Rust tests
cover the actual governor and process reservation counters, queued cancellation
and future drop, panic unwinding, and schema/configuration/branch invalidation.
The existing runtime admission model covers dynamic capacity/headroom semantics.

Implementation correspondence:

- `Prepare`: `RuntimeGovernor::try_admit` and
  `TokioRuntimeAdapter::prepare_blocking` admit control work before parsing.
- `Transfer`: `RuntimePermit::into_retained_memory` moves the existing process
  reservation and releases only execution resources; there is no release and
  reacquire memory gap.
- `AdmitExecution`: `RuntimeWorkRequest::retained_memory_bytes` augments hard
  capacity checks without charging already reserved memory again.
- `Retire`: prepared-state and retained-memory RAII owners release reservations
  after the AST/plan, including rejection, cancellation and panic.

Both owned Cypher requests and admitted SQL use these transitions. SQL retains
a schema-neutral template and validates its original database instance and
configuration before binding access paths against the execution snapshot.
Host-scorer callback ownership is separate from prepared plan memory: only its
descriptor participates in preparation. The model covers reservation ownership,
not callback behavior, SQL binding correctness, or cumulative output budgets;
request regressions own those checks.

Mem ordinary/streaming and report entrypoints share `read_execution` preparation:
the published graph view drops before execution admission, while the prepared
template keeps its transferred reservation. Read reports acquire the current
graph/resident or out-of-core pair only after admission and execute and record
telemetry outside the publication lock. Resident source capacity is separately
retained after releasing that mutex, without queuing or retaining execution slots. The existing transfer/admission
transitions apply unchanged. Native tests check writer progress in observation
callbacks and reject schema/instance changes; the model does not prove host
callback reentrancy or resident allocation sizes. `HawDBResidentReadLease` checks
the additional paired-capture/shared-funding/drop-order protocol.

Generated graph reads now use the same transfer/admission transitions. Their
additional exact-context-epoch and parameter checks run against the execution
snapshot before consuming the prepared plan. Native regressions distinguish a
reusable ordinary data plan from a stale generated context, check one preparation
lookup and refusal refunds, and allow metadata/generated reads while the direct
write owner remains held. Plain EXPLAIN uses the same read publication without
executing write plans; ANALYZE rejects write access. Captured query settings and
diagnostic budgets are native query contracts, not additional model states.
Schema guidance is metadata construction on a detached snapshot and does not
claim a prepared-query admission transition.

Standalone Mem search/hydration/retrieval helpers use the same resident lease
protocol: admission retains scalar/configuration metadata, `Capture` selects
the current pair, and funding happens after unlocking. They do not prepare a
Cypher template, so the preparation transfer transitions do not apply. Native
tests cover an old resident/out-of-core helper alongside writer publication and
fresh helper reads, and an admission callback that retires the old publication
or changes the projection I/O requirement. These add concrete correspondence
for existing model transitions, not new modeled states or throughput evidence.

Recommended verification:

```console
bazel test //docs/tla:HawDBPreparedAdmission_check
bazel test //docs/tla:HawDBPreparedAdmissionReleaseMemory_check //docs/tla:HawDBPreparedAdmissionKeepCpu_check //docs/tla:HawDBPreparedAdmissionIgnoreFloor_check //docs/tla:HawDBPreparedAdmissionKeepQueuedMemory_check
bazel test //docs/tla:HawDBPreparedAdmissionHardRefusalRecovery_check //docs/tla:HawDBPreparedAdmissionHardRefusalFailure_check
bazel test //crates/qos:hawdb_qos_tests //crates/runtime-tokio:hawdb_runtime_tokio_tests
```

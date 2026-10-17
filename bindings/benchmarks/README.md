# Ordinary host boundary qualification

These developer tools support [#976](https://github.com/nowledge-co/hawdb/issues/976).
They measure the existing owned Rust, Python and pure-Go APIs using the same
fixed-seed fixtures, parameterized Cypher, schema checks and ordered typed checksum.
They are reusable local tools; CLI arguments are not a production control plane.

```console
bazel build -c opt //:hawdb_bench_host_boundary \
  //bindings/benchmarks:python_boundary //bindings/go/cmd/boundary \
  //bindings/ffi:hawdb_ffi
python3 -B bindings/benchmarks/run.py --output /tmp/hawdb-boundary
bazel test //bindings/benchmarks:driver_tests
```

Stage the intended source first. The driver records HEAD, index tree and actual
producer hashes, and rejects unstaged changes and non-ignored untracked files.
Keep reports outside the checkout. Do not edit or rebuild during measurement.

The complete matrix uses 1,000, 100,000 and 1,000,000 rows, memory and file stores,
and all five cases: select, point, fill, fill_bulk and wide. It retains one
fresh-process discarded iteration and three measured iterations. Read workloads
seed in 512-row batches and warm the same query in the same database before
measurement. Point performs one lookup per input row in warmup and measurement;
fill commits one statement per row. Persistent writes retain SyncOnEveryWrite.
No limits or durability defaults are overridden. Refusal is a recorded failed
outcome, never a successful partial result or a latency comparison.

A small wiring check can use --sizes 3 --samples 1. It is not performance evidence.
The wide fixture includes Unicode, embedded NUL, lists, maps, booleans and NULLs.
Schema, ordered rows, type-aware checksum and phase timing must agree. Supported
fixtures do not prove every possible numeric, binary or UUID conversion.

query_boundary_ns includes parameter conversion, engine execution and result
conversion. For fill/fill_bulk it includes the verification read; the separately
reported write_boundary_ns and read_boundary_ns sum to it. consumer_ns measures
the host checksum. elapsed_ns includes that checksum and is not isolated crossing
cost. Process wall time and peak RSS also include parsing, fixture setup, warmup,
cleanup and the runtime. They are not query-only resource charges.

## Optional resource observers

```console
bazel build -c opt //:hawdb_bench_host_boundary_allocations \
  //bindings/ffi:hawdb_ffi_boundary_profile //bindings/python:_hawdb_boundary_profile
```

Pass the matching explicit artifacts and --native-profiles to run.py. Ordinary
builds do not install the allocator observer. Keep instrumented observations
separate from ordinary latency. Native counters count Rust allocator requests;
they exclude allocator rounding, foreign heaps and non-Rust workspace. Python
tracemalloc and Go heap counters cover their own runtimes, not the entire process.
Native parameter/engine/result timers do not include all host-side conversion.

On structured producer errors, failure_native_profile uses the distinct scope
producer_run_including_setup_warmup_and_cleanup. This wider window includes
setup, warmup and database cleanup. It is not comparable to a successful
measured-operation profile. Missing counters stay missing, never zero. Invalid
profiles or wrong scope fail validation. An error with valid resource observations
still fails the experiment and contributes no successful latency median. Startup,
process termination or failures before the observer can run may lack counters.
RSS includes runtime and fixture costs; no whole-process memory bound is claimed.

Go/C in-memory opening is an additive facade wrapper. Go treats the optional
symbol as unavailable on older libraries while preserving persistent opening.
The wrapper does not use a temporary persistent database for a memory workload.

## Immutable comparisons

compare.py builds both revisions before running either matrix, freezes producer
artifacts and Python runfiles, and verifies them before and after measurement.
Its historical baseline adapter is qualified only for the full immutable revision
3cdb2610ba8b984676b80ba22df64821787886d3. Its manifest records the exact adapter,
source, producer, runtime and selection hashes. Report ratios are baseline-first
ratios of medians, not alternating paired controls. A completed or green collection
is not proof of performance improvement, tail latency, or complete resource bounds.
Final acceptance still requires the requested complete workload and baseline scope.

All targets are manual developer tools. This change adds no workflow or fuzz CI
lane, retained/Arrow/node-view API, engine optimization, row-limit policy, durable
format change or stable Mem integration. No benchmark results are claimed here.

### Go host JSON conversion observations

The native FFI phase timers do not include Go parameter preparation/JSON encoding
or Go result copying/JSON decoding/normalization. Build the separate manual
producer to observe those costs through the same ordinary binding query path:

```console
bazel build -c opt //bindings/go/cmd/boundary:boundary_profile
```

Use this producer with `run.py --native-profiles --go-host-profiles` and the
matching explicit instrumented Rust, Python and FFI artifacts described above.
Without an explicit `--go`, the driver selects the profile producer when
`--go-host-profiles` is set. The ordinary target uses compile-time-false guards,
a zero-sized state and no timing/counter observer. The tagged artifact must not
be submitted as ordinary latency: the driver refuses unexpected host profiles.

`go_host_profile` reports parameter conversion, complete native-call and result
conversion wall times and call counts over the measured query loop only. Native
phase times nest inside `ffi_call_ns`; do not add the two. Lock acquisition,
host result consumption and fixture/input construction remain outside these
three phases. The existing outer query and consumer timers cover their stated
scopes separately. Counts must match all measured calls for the chosen workload;
missing, saturated, malformed or incomplete host counters cannot qualify samples.
Timing is instrumented observation, not an ordinary-latency result or a foreign
heap/whole-process memory bound. Snapshot formatting occurs after the existing
Go allocation observation window; that window otherwise retains its original
checksum/native-profile bookkeeping scope.

A failure after opening the database retains `failure_go_host_profile` over
query calls in setup, warmup, measured execution and cleanup, sampled after
Close. Close itself is not a query-conversion phase. Failures before a database
exists have no host snapshot. These errors remain errors and never enter
successful timing medians. Historical reports without host observers remain
missing; they are not retroactively completed. Before/after snapshots qualify
serial producers only; independently read atomic counters are not a coherent
per-operation snapshot for concurrent callers.

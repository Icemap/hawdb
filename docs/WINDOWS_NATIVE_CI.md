# Windows native CI

The native binding workflow uses seven independent Windows 2025 runners for
Bazel tests, plus an independent Cargo Clippy runner. There is no prerequisite
compile job or shard concurrency limit. GitHub runner capacity still bounds
actual parallelism. The existing `Windows native contracts` check aggregates
all shard results and Clippy, rejecting failure, cancellation and skipped work.

## Selection ownership

| Shard | Original Cargo selections | Profile / execution |
|---|---|---|
| transactions | `::serializable::` | facade all; one test thread |
| storage | storage and facade descriptor capacity; captured WAL admission; allocation/control-candidate/rollback union | storage all / facade all; one thread |
| search | generation spool; captured lease; Windows publication refusal | search all; one thread; refusal exactly one case |
| consumers | reclamation; stationary creation; stationary recovery; registered Consumer integration | facade all; one thread; creation exactly one case |
| identities | facade/executor/Cypher `identity_898_`; facade `_1043_` | facade default / component default; one thread |
| snapshots | portable, branch-open and unit-view identity | both facade default and minimal; exactly 1/2/3 cases respectively; original default threads |
| bindings | FFI retained; retained node; retained Arrow; complete Go tests | bindings / facade default / facade all; original default threads; Go cache disabled |
| independent Clippy | workspace, all targets, all features | unchanged pinned native Cargo toolchain |
| daily/manual abandonment | complete branch abandonment power-loss family | facade all; exactly eight cases; repository/default four or explicit 1/2/4/8 threads |

`scripts/run-windows-native-bazel.sh` owns the exact filters. Every Rust
selection must pass a positive number of tests; existing exact counts remain
mandatory. Filtered selections get separate stdout, commands, BEP and copied
native test logs/XML before another invocation can overwrite them. Failed
commands preserve their status and partial evidence. Test results are never
reused. The two-hour Rust/job ceiling and ten-minute Go ceiling are retained;
these ceilings are not run durations.

`windows_native_features.bzl` maps Cargo selections onto the same shared Bazel
libraries. It changes feature cfgs, never optimization or assertion settings.
The default `bazel` profile preserves existing targets. The optional telemetry
crate is explicitly resolved and linked only for the opentelemetry feature;
feature cfg parity alone does not prove optional dependency linkage. The Python 3.11+
manifest audit rejects feature inventory and forwarding drift, including root
default/minimal selections, Search defaults and storage dev dependencies.

## Cache layout and costs

The composite Windows setup restores Bazelisk, repository and external caches.
Disk actions use a separate `D:/hawdb-bazel-disk` root, capped at 2 GiB by Bazel
GC. Repository cache/output trees are not children of that root. No remote
cache is configured. Each shard writes its own immutable source/run key and
can restore earlier source/run or other shard entries. Including the run ID
allows useful compilation from partial failures to be saved on a retry. The
setup action's BUILD-only disk key is intentionally replaced with explicit
restore/save steps so source-only changes refresh cached actions.

Parallel cold runners can repeat compilation and consume more total runner
minutes and cache quota. Native Clippy keeps a separate Cargo target cache;
Bazel action caches cannot replace Cargo metadata. Shared library sources and
repository-wide checked compilation settings remain unchanged.

The previous serial [run 39969000144](https://github.com/nowledge-co/hawdb/actions/runs/39969000144)
at `469a0c0dacdae955c4fcb333a9286fdbe8f06c2f` recorded Clippy 2m26s,
serializable 8m59s, descriptor 4m32s and spool/captured work 3m33s before later
steps. These are combined build/test step durations, not isolated execution
measurements. They do not establish the new workflow's speedup. Compare final
head artifacts for cold and warm runs, separating queueing, analysis,
compilation, actual test execution, disk hits and upload/save time.

## Replay and runner checks

On a native Windows Git Bash terminal with the pinned toolchains:

```console
bash scripts/run-windows-native-bazel.sh snapshots
POWER_LOSS_TEST_THREADS=4 bash scripts/run-windows-native-bazel.sh abandonment
python scripts/check-windows-native-features.py
bash scripts/run-windows-native-bazel.test.sh
```

The helper regression uses a fake Bazel executable to verify native failure,
zero tests, missing BEP, exact-count mismatch and invalid thread selection.
It does not qualify Windows storage or compiler behavior. Actual Windows
runtime evidence belongs to the corresponding Actions shard at the final SHA.

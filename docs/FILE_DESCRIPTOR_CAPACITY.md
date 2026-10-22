# File descriptor capacity and internal progress

Issue Number: ref #819, ref #846

## Host configuration

`DatabaseConfig::max_open_files` and `WalReplayConfig::max_open_files` are
project-wide capacity targets, with a configured default of 1024. Their
`strict_capacity_limit` field defaults to `false`.

```rust
let config = hawdb::DatabaseConfig {
    max_open_files: 1024,
    strict_capacity_limit: false,
    ..hawdb::DatabaseConfig::default()
};
```

| Operation | Effective strict policy | At the capacity target |
| --- | --- | --- |
| Foreground reads and explicit reservations, default configuration | `false` | Evict idle immutable handles first, then admit accounted excess when active owners prevent eviction. |
| Foreground reads and explicit reservations, explicit strict configuration | `true` | Evict idle immutable handles first; if capacity is still unavailable, return `FileDescriptorError::BudgetExceeded`. |
| Ownership locks, mutable WAL handles, internal recovery/change capture/publication/checkpoint/cleanup admission | Always `false` | Preserve the operation's ability to finish while continuing to count every handle and reservation. |

This flag controls descriptor capacity only. It does not disable query memory,
result, disk, decoding, cancellation, ownership or durability checks.

Contexts for the same project share one ledger. Explicitly conflicting capacity
settings are rejected: `max_open_files` conflicts use `FileDescriptorError::ConfigurationConflict`,
and strict/soft policy conflicts use `FileDescriptorError::CapacityPolicyConflict`
with the original and requested boolean values. Component constructors without
a policy argument inherit the active domain. `DatabaseConfig` supplies its
boolean policy, including the default `false`; conflicting opens do not change
the active domain or its counts. Internal operations borrow the original ledger rather
than opening a second project domain or changing the foreground policy.
`FileDescriptorMetrics::strict_capacity_limit` reports the configured policy.
Open/reserved counts and high-water marks can exceed the target in soft mode.
Capacity calculations saturate available credit at zero when already over target.

## Pins and internal admission

An immutable native read retains an `Arc` for its complete I/O lifetime. Only
entries held exclusively by the cache are ordinary eviction candidates. Soft
admission does not authorize closing a live handle, deleting its immutable
object or retiring a snapshot's reachability root. Released handles become
eligible for later LRU eviction; releasing a pin is not immediate file deletion.

`FileOpenContext::internal_operation` and
`ProjectFileDescriptors::reserve_admission` create non-strict internal scopes.
Nested internal work can acquire additional accounted capacity when an outer
reservation cannot cover it. Internal contexts passed to joined workers retain
the active policy. Once the operation guard retires, later foreground work uses
the configured policy again. Long-lived native authorities retain project
identity without extending an expired operation reservation.

Registered Consumer staging and coverage sessions retain their original project
context and enter a fresh internal scope for each synchronous step. Idle tokens
retain neither descriptor reservations nor execution slots.

Registered Consumer generation cleanup and its explicit retry also enter fresh
internal scopes. They can reclaim retired generations while strict foreground
capacity is occupied, while retaining all native handles and deletion evidence
in the original project's accounting.

Owned projection-writer temporaries, stationary producer removal, captured-directory
publication leases, and Consumer publication/cleanup lease acquisition also enter
internal scopes. Captured leases borrow their original directory context; path
entry points include capturing their original directory. Strict foreground saturation cannot create
cleanup debt by itself. Native unlink, directory-shape, ownership and I/O
failures still retain evidence and require a retry; cancellation and memory
admission failures preserve the previous cleanup disposition.

Internal maintenance must not hold foreground execution resources while waiting
for another owner. This descriptor policy does not replace the separate CPU,
memory, physical-I/O or checkpoint state-machine progress contracts.

## Windows path identity probes

Windows here means the Microsoft Windows operating system. On Windows these
resources are native file/directory handles; HawDB accounts for them in the
same descriptor ledger used for Unix file descriptors.

Resolving an existing Windows directory can temporarily open a native directory
handle before the target project's identity is known. Such a probe must not
reserve a descriptor in every registered project: an exhausted unrelated project
must not reject the target open.

Namespace discovery instead uses a process-owned probe ledger. The existing
project-registry mutex serializes these short probes, so only one probe slot is
needed. The temporary probe closes before selecting the canonical project
domain. Canonical aliases still select the same domain and retain its configured
policy and conflict checks. This is separate accounting for unresolved identity,
not an uncounted open or permission to bypass a resolved project's strict policy.

This follows RocksDB's separation between temporary Windows identity probes and
database file-reader caching. Its
[`AreFilesSame`](https://github.com/facebook/rocksdb/blob/074134d98fb14e3ae1b81eb6dbab80dbf6dbdbd5/port/win/env_win.cc#L877-L950)
opens only the compared paths and releases their native handles with scoped
owners. Its
[table-cache setup](https://github.com/facebook/rocksdb/blob/074134d98fb14e3ae1b81eb6dbab80dbf6dbdbd5/db/db_impl/db_impl.cc#L262-L274)
is a separate policy; HawDB retains its own project-wide accounting contract.
RocksDB's [cache options](https://github.com/facebook/rocksdb/blob/074134d98fb14e3ae1b81eb6dbab80dbf6dbdbd5/include/rocksdb/cache.h#L140-L145)
also default to non-strict capacity when entries remain pinned.

## Operating-system limits

Unix `RLIMIT_NOFILE` is process-wide and shared by threads. HawDB reads it with
`getrlimit`; production code does not call `setrlimit` or alter host startup
configuration. Project acquisition retains the existing OS-capacity preflight
and host-headroom policy. `strict_capacity_limit=false` does not raise the OS
limit or suppress native exhaustion and sharing errors.

Unix uses one process-wide ledger for HawDB's observed OS allowance: the soft
limit minus 64 host descriptors. Every project, standalone file operation and
namespace probe shares it. A new project registration refreshes the observed
limits without discarding already charged handles or reservation credit.
Per-project strict targets remain independent; they do not each receive a copy
of the process allowance.

Soft foreground and internal admission may exceed a project's target, but all
new slots must fit the shared process allowance. After local idle-cache eviction,
exhaustion returns a typed `OsLimit` before native open or reservation mutation.
A prepaid slot is charged once: converting it to an open handle does not charge
again. A failed open returns credit to its active reservation; closing an
unreserved handle or retiring unused reservation credit releases the process
charge. Another project cannot consume an operation's prepaid slots. Strict
foreground target refusal continues to return `BudgetExceeded`.

The process ledger covers managed HawDB resources, not a kernel reservation or
all host activity. Untracked host pressure beyond the headroom, changes to OS
limits between observations, and native sharing or I/O failures can still reject
native opens. Such external failures do not authorize uncertain durable effects;
those still fail closed and retain recovery evidence. The 64-descriptor host
headroom is not a separate internal pool.

RocksDB likewise
[reads the current soft limit](https://github.com/facebook/rocksdb/blob/074134d98fb14e3ae1b81eb6dbab80dbf6dbdbd5/port/port_posix.cc#L237-L251)
and, for finite settings,
[adjusts its configuration](https://github.com/facebook/rocksdb/blob/074134d98fb14e3ae1b81eb6dbab80dbf6dbdbd5/db/db_impl/db_impl_open.cc#L54-L62).
The embedding host owns any process-limit increase.

## Verification scope

The storage descriptor family covers soft overshoot with pinned readers, strict
foreground refusal, nested internal progress, worker handoff, restoration of the
foreground policy, alias identity and reservation/handle retirement. Facade
regressions cover low-target open/commit/checkpoint/reopen and maintenance while
strict foreground capacity is occupied. Existing refusal fixtures explicitly
opt into strict mode. A Unix subprocess lowers only its own soft limit and
saturates the shared allowance across three projects in both soft and internal
modes. It verifies typed refusal before raw exhaustion, preserved host headroom,
standalone/probe accounting, cross-project prepaid-slot protection, native-open
failure retirement, atomic cross-project admission of the final slot, and
complete shared-credit reuse.

See [the test index](TEST_INDEX.md) for owning targets and execution lanes.
The `windows-native-contracts` job in
[Native Bindings](../.github/workflows/native-bindings.yml) owns native Windows
strict Clippy, the complete descriptor family and the facade capacity tests.
Their source/toolchain records and test output are retained as CI artifacts.
Source inspection and cross-target compilation do not establish native Windows
runtime behavior, exact process-wide handle counts, namespace durability or
machine power-loss safety. Record those results separately on the tested source.

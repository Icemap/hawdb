# Root process custody and concurrent reference retirement

## Required native contract

A retained immutable child or admitted read must keep captured directory and
native process ownership alive after its producer closes. It must not retain
that producer's mutation slot: a successor can publish against the same root
while old sources stay pinned. Branch existence remains durable and independent
of these runtime resources. Closing a root reference deletes no branch or file.

The proposed registry uses separately admitted entries and
`hawdb_core::SharedAllocation`, with no escaping raw Arc or Weak references.
Lookup compares a cached physical identity while its counted directory remains
live. That identity is not a logical incarnation token. Initializing and
retiring entries exclude reacquisition while native I/O happens outside the
global registry gate. Native resources close before the entry is removed.

This last ordering matters even after failed initialization: under conservative
POSIX process-owned record-lock semantics, closing *any* local descriptor for a
lock inode releases the process lock. Removing an entry before closing its old
descriptor allows a successor to acquire a new lock and then lose it when the
old descriptor closes. A native directory handle alone does not protect this
lock lifecycle. Actual platform lock behavior still needs native qualification.

## Two finite models

`HawDBRootProcessCustody` separates initialization, native open, failed
initialization, producer closure, admitted mutation, independent child,
in-flight read, successor admission, retirement, native close and unregister.
Two local lifetime identities and one unsupported peer are sufficient for the
early-unregister counterexample. The positive standalone exploration has 991
generated / 354 distinct states, empty queue, depth 15. Its invariants require:

- live producers, mutations, children and reads retain the native lock;
- admitted mutations retain the producer slot even after producer closure;
- children do not retain the producer slot after mutation completion.

Six independent controls violate NativeCustody (early unregister, closing the
root with the producer, ignoring a read, or using only local exclusion),
MutationOwnsProducerSlot (early slot release), or ChildDoesNotOwnProducer
(retaining the producer in a child). Each standalone control exits 12 at its
named invariant, not merely at an arbitrary TLC failure.

`witnesses/HawDBRootProcessCustodySuccessor.cfg` disables all unsafe controls
and adds the deliberately false `NoSuccessorWithRetainedChild` invariant.
Its counterexample reaches a successor producer with the old child alive
(102 generated / 61 distinct states, depth 7). This is a reachability witness,
not a safety mutant or a liveness proof.

`HawDBRootReferenceDrop` addresses a separate concurrent-drop bug in a naive
reference-count implementation. Starting with the registry and two external
references, both droppers can observe more than two references under the gate,
then both decrement outside it, leaving a ready registry with no external
reference and native resources never retired. The positive model has 9 generated
/ 8 distinct states, empty queue, depth 5. The off-gate control reaches
`ReadyHasExternalReference` failure (13 generated / 12 distinct states).

The intended implementation therefore decrements a **nonfinal** reference under
the same gate as the count check. The registry and another external reference
guarantee this cannot destroy its payload under the gate. The last reference
instead marks Retiring under the gate, closes native resources outside it, and
unregisters only after close. SharedAllocation's final control-block and payload
destruction also happens outside the gate. All clones and drops must follow
this private protocol; ordinary Arc access would invalidate the argument.

## Evidence and correspondence limits

The counts above come from standalone TLC 2.19 runs with the repository's
pinned tool artifact. Both positive models, seven controls and the successor
witness passed registered Bazel replay (10/10 targets). All 70 materialized
module/configuration/toolchain/result/log files were archived and hash-checked;
each counterexample was inspected for its named invariant and exit 12. The
registered positive graphs again contain 354 and 8 distinct states, respectively.
Parallel exploration changes counterexample exploration counts and reported
depths; the standalone counts above describe those original runs only.
The witness is excluded from both the positive suite and the mutant manifest.
`scripts/check-storage-tla.sh --check-mutants` additionally checks the specifically
named violations for the seven safety controls.

Neither model is a refinement proof of Rust. They abstract reference operations,
initialization and native lock transitions as atomic, and omit allocation
failure, poisoned locks, platform sharing modes, read-only upgrades, artifact
registrations, identity reuse, storage crashes and scheduling fairness. The
reference model assumes no new clone after final retirement begins; native
lookup/clone exclusion must establish that premise. The process model assumes
exact owner presence rather than implementing reference counts. Composing the
two models with SourceRootCustody and actual GC remains required.

The native SourceDirectory primitive supplies counted relative opens, lock-file
opens and cached physical identity; it does not implement this registry or any
reader/GC registration. Supported native power-loss contexts now have a private
relative-directory trace; [its implementation and qualification limits](RELATIVE_DIRECTORY_IMAGE_DESIGN.md)
remain separate from these finite models. Unbound/foreign contexts still refuse.
Full source incarnation, exact artifact
pins on both cleanup paths, complete C-ready/E activation, managed registry
capacity, actual Mem adoption and writer/fairness/resource/power-loss gates
remain open. These models do not authorize governed serving or stable release.


## First private native registry implementation (verification pending)

`crates/search/src/out_of_core/root_registry.rs` now implements a counted
registry used by the private admitted-root constructor. Ordinary readers keep
their existing path and admitted roots still refuse conversion to public serving.
The registry uses a per-entry admitted LinkedList node and the existing
SharedAllocation owner; its shared state retains query/host memory without an
execution permit. Ready lookups reject a different project descriptor domain.
SourceDirectory retains only the project domain, while later operations supply
fresh reservation inventories.

| Model transition or obligation | Native implementation |
| --- | --- |
| BeginInit | Insert Initializing entry before opening the process lock |
| OpenNative / FinishInit | Relative nontruncating lock open and try_lock outside registry gate, then Ready |
| FailInit | RootRef initialization guard follows the same retirement path on error or unwind |
| Nonfinal reference drop | Count check and nonfinal Shared decrement under the registry gate |
| BeginRetirement | Last external reference changes phase to Retiring under gate |
| CloseRetired | Extract resources, explicitly unlock successful process custody, then close lock FD and directory FD outside gate |
| Unregister | Unlink entry only after native resource closure; free receipts/control block outside gate |
| Producer and admitted-mutation slot | Not implemented by this registry |
| Exact child and in-flight source pins | Not implemented; children must gain independent custody before activation |

The new native cases are written for shared/reopened descriptor reuse and final
refund, 32 concurrent two-owner drop interleavings, both retirement windows,
FD-denied initialization, cancellation, cross-domain rejection, peer exclusion
after producer execution ends, and fresh operation admission with the old root
retained. The first 14-case root/constructor campaign initially failed one
fixture because its shared directory helper already retained a default-1024 FD
domain while the probe requested eight. Creating an independent eight-FD project
fixed the fixture; all 14 cases then passed. This changed no production quota or
assertion. Source-custody regression probes still require both GC paths and exact
child registrations.

The new `.search-source-process.lock` is a cooperating-process protocol. Publication leases now acquire or borrow this custody, and pass their actual
authority into cleanup. A hostile replacement of its namespace entry is not
covered. See the [pending/exact registration refinement](SOURCE_ROOT_CUSTODY_DESIGN.md). No interoperability, incarnation or GC safety
is inferred from holding this file. Read-only lock acquisition/upgrades,
complete source-relative publication/GC integration and native power-loss bridge
qualification remain required. Do not activate governed serving until these paths and original
qualification gates are complete.


## Captured head reads (private construction only)

RootRef opens the manifest relative to its counted directory and returns a held
file. Private construction installs pending capture before opening that inode,
then closes its owned publication lease before bounded read/decode. Opening returns its I/O wave
before read admission, so a one-slot operation cannot self-wait. The obsolete
pathname opener was removed. Existing held-file replacement, cancellation and
pre-allocation limit guards exercise the actual held-file entry point; the
large-allocation observation still includes native open and cancellation.

New native guards replace the root pathname with a foreign directory containing
a same-length different head, verify the captured bytes and unchanged foreign
bytes, and check descriptor/memory/I/O refund after size and FD denial. These
checks, the seven root-constructor cases and six read-admission guards passed
**22/22**, zero ignored, 950 filtered, on macOS arm64 / Rust 1.97.1. Strict Search
all-target/all-feature Clippy passed. The changes were tested against preceding
head 8378f38; fast-forwarding the test-only 11c4938 changes neither Search nor
Storage source inputs.

Only the root head uses this captured-directory path so far. Segment/run loads,
ordinary independent-child capture, fully source-relative GC and logical
incarnation still need integration. Private exact pins and pending captures are
now implemented, while native recorder mapping has separate scoped evidence. The admitted-root
conversion fence remains in place; these results authorize no public governed
serving or complete source-custody claim.

## Cold registry gate allocation (qualification in progress)

The first unfiltered native Search campaign exposed an actual scope gap in the
focused single-thread runs: **928 passed / 21 failed / 20 ignored**, with every
failure observing the same 64 live measured bytes. The first measured admitted
root initialized the process-static `std::sync::Mutex` on macOS. Its native
backing allocation stays with that static owner after all root entries close;
subsequent complete-reclamation guards correctly continued to reject it.

A standalone Rust 1.97.1 diagnostic using the same allocation tracker confirms
64 retained bytes for the first static Mutex lock. A local Mutex allocates its
own backing and releases it on destruction, leaving just the static 64 bytes.
This is not evidence that the per-root retirement ordering leaked an entry.
Neither warming the registry outside measurement nor subtracting 64 bytes from
the assertions establishes a complete cold governed-construction contract.

The proposed fix gives the process-static registry an Apple `os_unfair_lock`
word in static storage. It calls the exported public lock/unlock functions;
it does not implement a spin loop or copy OS internals. Other platforms retain
`std::sync::Mutex`. The Apple guard requires a `'static` gate, preventing moves
after first use, and cannot cross threads. Exclusive guard borrows protect the
UnsafeCell payload; Drop unlocks on the acquiring thread, including unwind.
Native resources, final payload/control-block destruction and entry receipt
release still occur outside registry exclusion. The nonfinal decrement stays
inside the same critical section as the count check.

This refines the finite models' existing mutual-exclusion premise without
changing their reference/retirement transitions. It adds no scheduler fairness
or liveness guarantee. OS lock behavior, contention/resource qualification and
all original MVCC gates remain explicit requirements. A new cold-lock heap guard
and the existing full-reclamation assertions must pass without relaxed budgets,
changed assertions or excluded first-use initialization.

The native word layout and zero initializer were checked against the pinned
local macOS SDK `os/lock.h`. Apple's public contracts describe
[the initializer](https://developer.apple.com/documentation/os/os_unfair_lock_init)
and [blocking acquisition](https://developer.apple.com/documentation/os/os_unfair_lock_lock).


## Native Windows replacement cut

Head 1c1d302's Windows directory/image selection passed all 29 cases. Its
root/constructor/read selection passed 22 and failed one before the library
read: the fixture's ancestor rename was refused while the process-lock child
file was held. Source line 108 identifies the host rename, not a head-read or
registry failure. The raw native adverse result is retained.

The Windows head test now replaces the pathname at the existing AfterInitializing
hook: the counted directory has been captured and the Initializing entry is
installed, but the child process lock has not yet opened. It requires that the
subsequent process-lock file is created in the captured moved directory and is
absent from the foreign replacement. Then the original same-length foreign-head,
held-head bytes, one-slot I/O, descriptor count and memory refund assertions all
apply. Unix keeps the original replacement after complete initialization and
process-lock acquisition. No production code, access mask, lock lifetime,
budget, deadline, test selection or final postcondition is relaxed. This checks
relative authority at a native supported replacement boundary; it does not
promise that Windows permits every host rename while descendants are held, and
does not qualify segment/run loads, source pins, GC, incarnation or activation.

## Inherited open descriptions at final closure

Closing a parent FD alone can leave an inherited flock description locked until
exec/exit. Root and native-only publisher process custody now use the storage
library's owned lock guard, constructed by `File::try_lock_owned` only after a
successful acquisition. Guard destruction explicitly unlocks before native close,
matching the existing storage publication guard's lifetime rule. CLOEXEC remains
set but is not the unlock mechanism. A borrowed publisher never constructs a
second process-lock owner.

The publication-custody model and its `CloseWithoutUnlock` control now exercise
this case; see [the scoped model/native correspondence](SOURCE_ROOT_CUSTODY_DESIGN.md#publication-and-retained-root-process-lock-coordination).
The finite model assumes native unlock succeeds. Unlock errors fall back to
close and may leave temporary exclusion until inherited descriptions close;
no scheduling/latency guarantee or native refinement is claimed from this model.


## Native database directory ownership

`DatabaseDirectoryLease` now retains a counted captured directory and opens
`owner.hawdb.lock` relative to that exact root. The reserved database lock name
has a crate-private entry point; the public hidden-lock component validation
remains unchanged. Both ownership descriptors use the original project domain.
A live physical identity keys the process registry, so a renamed directory or
alias cannot cause another local process-lock descriptor to be opened. Binding
checks used by branch metadata/root-pin code compare the actual held directory,
not a saved canonical string. That check is a fence, not permission to use paths
afterward or proof of a persistent logical incarnation.

A local registration guard handles all fallible initialization/unwind paths.
Field/drop order is lock close, process unregistration, then directory close.
The directory identity therefore stays pinned throughout registry membership;
a late old process-lock close cannot release an admitted successor's lock.
Explicit unlock still precedes close, preserving inherited-description behavior.

The finite process-custody model's `CloseRetired` before `Unregister` ordering
applies to this native close boundary. Positive and `UnregisterBeforeNativeClose`
control were executed through their registered Bazel targets. This mapping does
not claim that the model covers namespace aliases, FD budgets or the native OS
syscall behavior; actual native regressions cover those separate obligations.

Native tests exercise FD refusal before lock creation and retry in the same
project; namespace replacement after capture but before lock open (including the
Windows-compatible empty-directory move); real cross-process exclusion; alias
refusal with a live renamed Unix directory; and injected unwind after actual
lock close while registration still excludes reacquisition. The unwind test
requires the exact injected panic payload so an assertion failure cannot count
as its expected unwind. Full Storage and compiled counter-control results are
recorded separately; narrow native passes do not qualify every platform.

This is real adoption by durable store ownership and branch guards, not the
Consumer allocation ledger, complete captured GraphStore data I/O or stationary
activation. Those contracts, original resource/recovery/fairness gates and actual
Mem adoption remain required. The extra retained directory FD is charged, not
hidden in a separate allowance or removed to make a tiny budget pass.

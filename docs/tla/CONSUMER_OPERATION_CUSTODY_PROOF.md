# Native consumer preparation custody

This component implements target-operation exclusion and clean-checkpoint
admission for the future C-ready/E session. It does not implement that session,
private generation installation, source/binding incarnation validation, a new
read-view/head format, operation receipts or activation. The
[installed-activation design](CONSUMER_INSTALLED_ACTIVATION_DESIGN.md) and
[receipt-ledger design](CONSUMER_RECEIPT_LEDGER_DESIGN.md) remain proposals.

## Source correspondence

[Consumer operation authority](../../crates/search/src/consumer/operation.rs)
contains the actual native publication lease and one AtomicBool, retained through
an Arc. Its non-cloneable preparation token shares that existing authority;
reservation does not create a second memory ledger or governor. Successful CAS
admits exactly one target operation. apply, checkpoint and directory publication
acquire the same slot before invoking a callback, changing checkpoint_uuid or
creating/moving a directory. Busy is a typed `HawDBError::ProjectionBusy`.
Marker writes already require owned mutation, so a shared index view cannot
bypass apply's guard. Public cleanup either runs inside that guarded owned scope
or must acquire the retained native root lease and defer. Cache-only reads do
not grant artifact publication authority.

The token can move to a worker and outlive its ConsumerProjection. The last
owner/token, not just the consumer handle, releases the actual root lease.
Durable registration remains until explicitly removed.

[The facade admission](../../src/api/search_projection_consumer/mod.rs) validates
the source namespace, registration/projection identities, recorded completed
receipt and current consumer state before granting custody. It reserves no
source snapshot and advances no watermark. Catch-up checks availability before
hydration and acquires the operation again before mutation. Current canonical
source writes remain independent of target custody. Actual Mem must handle Busy
as deferred maintenance, never turn an already committed canonical write into
an ambiguous application failure; that host integration is not implemented here.

A complete old target D can be retained even when the legacy feed expired or
its delta history was trimmed. Fresh preparation admits those two readiness
reasons only after the checkpoint/identity checks; it does not clear them.
Ordinary delta catch-up still refuses. Future publication must construct a
complete fresh-C replacement and qualify C..E itself. Corrupt or uncertain
checkpoint state, source rewind, mismatched identities and untracked mutation
are not authorized as a delta repair. This permission is not fresh artifact
integrity validation or target-incarnation proof, and cannot bypass the generic
writer's registered-root rejection.

## Clean-checkpoint argument

Source-epoch equality alone is insufficient: an owned callback can change a
resident document without advancing the source epoch. The existing freshness
report can then still say there are no uncheckpointed changes. ConsumerProjection
now records pending_mutation before entering any mutable callback or fallible
checkpoint. An error or panic keeps it set. A completed successful checkpoint
clears it only after storing its receipt; a successfully validated reopen starts
clean. Directory publication preserves earlier pending state on success and
retains uncertainty on failure.

Reservation requires both that flag to be clear and the completed receipt to
match the current binding, applied epoch and durable epoch. A fallible checkpoint
which changed checkpoint_uuid before failing cannot satisfy that predicate.
Restoring an older file alone cannot certify its in-memory state. Failed
reservation drops its local operation guard, allowing a later explicit repair;
it cannot strand the operation slot. An opaque callback's error is treated
conservatively even if it happened to make no mutation. Native root/registry
reconciliation after an uncertain checkpoint remains separate from this token.

Conditional on conforming native lease users and safe Rust ownership:

1. While a preparation token is live, its successful CAS has not been released.
   Every protected mutator therefore returns Busy before side effects.
2. Its Arc retains the same native file-lock/registration object, so consumer
   closure alone cannot permit another opener or a standalone cleanup sweep.
3. Without an admitted mutation or checkpoint, the guarded clean binding/receipt
   state cannot be changed through the consumer's protected methods.
4. RAII releases the slot on admission failure, cancellation and unwinding; root
   exclusion persists while either consumer or token still owns the authority.

This is a Rust source/lifetime argument and native regression evidence, not
mechanized refinement. It supports part of the proposed activation model's
SlotExclusion and CleanAdmission premises. That model's C-ready, source scope,
selector/receipt, durability, unknown-outcome and heavy-custody transitions are
not implemented by this component. Its finite checks must not be presented as
proof that the full native activation path exists.

## Qualification status

The first native vector-profile consumer selection passed 11/11 tests; a
strengthened real checkpoint-publication failure fixture passed 11/11 again.
The final minimal selection passed 10 tests with 713 filtered (2.26 seconds).
The full Search vector library passed 881 tests, zero failures and 20 existing
ignored tests (317.33 seconds), including cross-worker token release after owner
closure. Only two facade receipt-policy/test files changed afterward; a complete
tree comparison verified Search/core dependency bytes stayed fixed. The final
facade selections passed 25 tests in both default (1,981 filtered, 118.35 seconds)
and minimal (1,865 filtered, 769.98 seconds) profiles. The minimal run remained
live during a long source branch-checkpoint rename syscall; its sample is retained.
These timings are observations under concurrent load, not benchmark evidence.

Final Bazel selections passed kernel consumer tests 11/11 (890 filtered), facade
consumer tests 25/25 (1,981 filtered), native portable facade 5/5 and WASM bridge
7/7. Strict locked workspace/all-target/all-feature Clippy passed. Target WASM
strict Clippy passed for the library/in_memory_portable and playground example;
the final WASM example linked with the documented Clang/pinned LLVM archiver.
No browser run is claimed. The three required unfiltered local fuzz suites
passed 98/98 targets, 96 executed and two cached (4,226.975 seconds wall,
945.86 seconds critical path). Their 294 log/XML/cache-status files are retained.
Runtime/test/model tree `5905acf20bd85b9c9febe57ed8cae11a3d3cfe7d` stayed fixed
through qualification. The primary differs only in four Markdown design/proof
files, including the newer cleanup proof retained from its own qualified commit.
Final primary formatting, strict workspace Clippy and all-file prek checks passed;
installed commit-hook results are recorded with the published component receipt.
The 27 proposed-protocol model/configuration pairs and expected outcomes were
compared with the retained direct/Bazel evidence; they do not qualify a native
activation implementation. These runs used pinned Rust 1.97.1 on macOS arm64.

The first strict-lint runs found required exhaustive error classifications for
ProjectionBusy, including the WASM bridge; these were added without suppressions.
An initial broad Bazel attempt was deliberately interrupted before executing a
test to refine expired/trimmed-feed admission. It is excluded from final-source
qualification. An initial expired-reason pattern omitted its variant fields and
was corrected before final qualification. No assertions, budgets, timeouts or
durability defaults were weakened.

An initial primary all-file prek invocation saw concurrent index staging as a
file change even though its Clippy command exited successfully. A complete
source/index comparison found no runtime change, and the stable-index repeat
passed both hooks. That rejected invocation is retained as adverse evidence,
not substituted for the final successful checks.

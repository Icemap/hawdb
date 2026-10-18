# Deferred project capture handoff

## Native transition

`ProjectMetadata::into_capture` moves its original `Arc<SourceDirectory>`,
selector and `ProjectFileDescriptors` into `CapturedProject`, dropping the
catalog image. It performs no native open, allocation of a new descriptor
domain, or lock acquisition. Cloning shares the existing directory receipt.
`CapturedProject::metadata` reads fresh selector/catalog bytes through that
same capture and requires the original selector. A replacement's matching
UUIDs cannot authorize those reads.

The facade now stores this capture separately from deferred configuration in
`BranchRuntimeCell`, instead of
closing the metadata root and reopening its pathname at first data access.
The cell retains the project after admission and shares it with a branch
selection candidate before exchanging runtimes, retaining one fixed project
directory for the facade lifetime; it does not acquire one directory per cold
branch. This adds one honestly charged persistent slot relative to the previous
facade. The busy-main test accounts for that slot explicitly. Refresh shares
it and retains the original global FD cap and admission inventory.

Before entering the still-path-based default-branch recovery kernel,
`require_original_namespace` rejects a physical replacement. No branch runtime
is installed on this refusal. Explicit pending-create recovery and abandonment
reuse the same capture for metadata instead of acquiring a second root; their
legacy pathname entry points use the same fence. Restoring the original namespace permits retry
using the same pending capture. On Windows the namespace comparison itself
acquires a temporary directory slot in the original descriptor domain.

## Finite safety argument

[HawDBDeferredProjectCapture](HawDBDeferredProjectCapture.tla) models snapshot
transfer, namespace replacement/restoration, entry fencing, captured metadata
refresh and refusal retry. Both roots may have equal project/main UUIDs.
`Transfer` preserves `held = 1`; all other correct transitions leave it unchanged.
`Refresh` obtains metadata from `held`. A passed fence records only the physical
root actually observed at that check. Thus `CaptureRetained`,
`MetadataFromCapture` and `FenceObservedOriginal` follow inductively.

The registered positive TLC configuration explored **19 generated / 10 distinct
states, depth 5**, with an empty remaining queue. Three generated controls
execute the intended invariant violations:

| Control | Violated invariant |
| --- | --- |
| DropOnTransfer | CaptureRetained |
| RecaptureForAdmission | MetadataFromCapture |
| SkipNamespaceFence | FenceObservedOriginal |

All four Bazel targets executed. The model allows namespace replacement after
a passed fence. It deliberately does not assert that legacy pathname recovery
is race-free or that passing the fence grants authority to a later pathname.

## Native checks and remaining authority work

The storage regression transfers the snapshot, clones the capture, installs a
valid same-selector replacement, rejects that namespace, and reads the original
catalog through the shared handle. Restoring the original namespace permits
retry; only dropping the final shared capture releases its counted slot.

The facade regression replaces a closed, checkpointed project with a complete
valid clone after deferred open. Two attempted queries must reject with the
physical-replacement integrity error, preserve replacement catalog bytes and
admit no runtime. Restoring the original directory must recover its data.
Busy-main retry, exact source-budget exhaustion, an isolated native FD census
over 64 cold branches and the full branch-project suite cover accounting/lifecycle
interactions. The native census expects one fixed facade directory, no growth
per branch, and return to the baseline after facade closure. Independent native CI selections include these regressions. A separate Unix
regression exchanges the branch runtime, replaces the namespace with a complete
clone, and requires explicit recovery to reject it through the retained capture.
Windows does not permit that live-branch enclosing-directory move; the earlier
deferred boundary exercises replacement without a live branch lock there.

This closes the facade's discarded-capture/reopen gap, not the full kernel
custody obligation. Catalog/head/object/mount/data/WAL writers still include
pathname operations. A replacement after the entry check can race those
operations; no claim of zero foreign effects, complete C..E authority, persistent
Consumer incarnation allocation, stationary publication, or power-loss proof is
made here. Those require handle-relative kernel operations and the independent
durable ledger described by the
[stationary Consumer contract](../specs/CONSUMER_STATIONARY_PUBLICATION_SPEC.md).

## Kernel metadata entry

The follow-on [kernel metadata and UUID ownership argument](BRANCH_ADMISSION_CAPTURE_PROOF.md)
covers original-domain admission, captured catalog locking, UUID ownership and
head reads. Object/WAL/runtime/root-registry paths remain outside that refinement.

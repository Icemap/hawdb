# Branch descriptor admission accounting

## Actual operation and accounting boundary

Branch admission retains the original project descriptor domain and keeps the
currently selected runtime alive while validating its replacement. Its native
base inventory is three directory/ownership-lock pairs (UUID, disposable runtime
and root registry), one mutable WAL slot, and four sequential metadata or
publication temporaries: `3 * 2 + 1 + 4 = 11`.

This is the **admission floor**, not a maximum descriptor claim for arbitrary
recovery. Additional retained checkpoint/index artifacts must still acquire
charged permits. Large candidates may refuse during recovery; they never gain
an independent descriptor allowance or an uncounted fallback. Ordinary query
read waves run after storage admission and acquire their own native capacity.
An admission reservation cannot fund a query after that reservation has ended.

The former preflight reserved `4 + 16 + 4 = 24`: the old four-handle base, a
maximum query worker wave and four temporaries. It omitted the newly retained
owner directories while reserving the unrelated query wave. Actual diagnosis at
32 descriptors found nine open, zero reserved, zero cached, three ownership
locks and six transient handles. The preflight requested 24 with only 23 free,
before target recovery. The native call stack points to the facade's source
`reserve_target_admission_resources`, not a nested quota or missing cache
retirement. Reserving the actual 11-slot floor admits that source/target pair.
The host's configured/effective cap stays 32.

## Safety argument

Let `P` be charged open-slot permits and `R` reserved capacity in one project.
`P` conservatively includes certain temporary filesystem operations, so it is
not an exact count of OS handles. The existing `BudgetState::reserve` transition
admits only when `P + R + requested <= limit`; `DescriptorPermit` transfers one
slot from reservation to charged ownership. A managed native file closes before
its permit refunds capacity. Under the native coverage obligation
`managed descriptors <= P`, changing the floor preserves both `P + R <= limit`
and the managed-descriptor bound. It changes no charging, cache eligibility or
namespace authority. This does not bound unrelated host-process descriptors or
prove that every OS/filesystem backend implements that coverage correctly.

The floor protects the base admission phases. It does not promise that all
later recovery reads fit it. Additional opens continue through the same global
cap. A descriptor refusal before target construction creates no runtime; a
refusal after recovery returns no candidate for installation. The storage
kernel keeps the target UUID owner outside recovery and through cleanup. Only
complete recovery, metadata/binding revalidation and root registration return
an admitted store. The facade replaces its selected runtime only after complete
candidate validation. Old reader/branch custody is retained independently.

The unchanged [native-lease model](HawDBAdmittedNativeLease.tla) and its
`SkipCharge` control are executed separately. That model assumes managed FD
admission and proves counted lease custody in its finite scope. The executed
positive graph has 965 generated / 385 distinct states, depth 16; `SkipCharge`
violates `DescriptorCovered` after two generated/distinct states. It does not
model this floor, branch selection, runtime cleanup, physical OS limits or
query parallelism. No composed MVCC or branch resource proof is inferred from
its pass. The counting argument above relies on the actual permit transitions;
native regression evidence is recorded in the PR.

## Native obligations and limits

The unchanged 32-FD inventory and live-writer/reclamation regressions must pass.
A dedicated facade test holds 24 non-cache slots, leaving at most eight slots
after legitimate idle-cache eviction: catalog lookup fits, admission refuses before creating the target runtime, source identity/epoch and catalog/
head bytes remain unchanged, and the same cap permits retry after blockers close.
The source remains writable; the child still reads its original inherited data.
The old 12-FD cold-creation refusal expectation now legitimately succeeds. A
separate positive test requires idempotent cold creation under that same limit;
the refusal case opens an existing project with eight slots (separate from
one-time empty-project bootstrap) and still checks no catalog mutation, no data
runtime and no retained native descriptors. The 64-unopened-branch qualification
keeps its original 32-FD cap and native process descriptor measurements.

A kernel test exhausts the actual project after target recovery but before
revalidation. It requires typed refusal, cleanup while the UUID owner still
excludes another opener, unchanged catalog/head, a usable source and successful
retry. After owners close, any remaining descriptors must be charged idle cache
entries; actual idle eviction must close them before the final zero count.
Closing a busy capture or dropping a root to pass preflight is not allowed.

This fixes the native branch admission floor, not the Consumer allocation/birth/
debt journal or stationary serving. Full C..E/source ownership, lost/torn/reordered
write recovery, actual physical memory/I/O and multiwriter/fairness/group-fsync
qualification remain required. Windows metadata recovery separately confirmed
five-slot exhaustion while retaining its publication candidate through rename;
[the metadata argument](PROJECT_METADATA_CAPTURE_PROOF.md) derives the platform
peak and states its native qualification limits. This branch floor argument
does not waive that recovery gate or change synchronization.

## Captured metadata entry

The captured branch entry borrows the original `ProjectFileDescriptors` and
preserves its configured-limit conflict check without consulting a saved path.
Its relative catalog lock/read reuses one branches-directory capture; selector
and catalog files open sequentially. UUID ownership consumes its already captured
directory instead of reopening it. These operations still charge the same global
cap and reservation inventory. See the [separate custody argument](BRANCH_ADMISSION_CAPTURE_PROOF.md);
this does not make the branch-admission floor a bound on arbitrary recovery or
turn the remaining pathname effects into source-qualified operations.

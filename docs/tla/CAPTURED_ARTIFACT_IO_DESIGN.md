# Captured artifact I/O and cleanup

Status: CI integration draft. Private exact pins, captured immutable-file
reopen, full-reference projection validation and captured projection deletion are
implemented in the integration source. Ordinary governed serving remains
unqualified. A stronger native regression still fails because temporary
validation stages and lexical spills use the replacement pathname. Captured
scratch creation, spill I/O and deferred cleanup custody remain required; the
existing projection-file isolation probes do not establish that boundary.
These contracts do not complete MVCC qualification.

## One authority for the complete operation

A source is a logical root incarnation with immutable artifact names, backed by
a retained physical SourceDirectory and cooperative process-lock custody. Its
physical native identity alone is not a persisted incarnation or historical ABA
proof. Capturing a named directory once does not bind later File::open calls.

Private construction must install pending capture under the actual publication
lease, open the chosen head relative to that capture, then release only its
owned short publication lease. Decode, checksum validation, payload/layout,
lexical, vector and mutation-run reads must all use that authority. Transfer
pending protection into independently owned artifact pins before returning.
Failures refund the constructed prefix and clear pending protection; a live
publisher may legitimately retain successfully promoted root custody.

The cleanup operation borrows the actual successful publication lease for its
whole lifetime. A typed internal cleanup context must contain that captured
source, rather than accepting an independent Path plus generation sets:

1. Open and validate the active selector and its complete referenced closure
   through the captured directory. Keep the current checksum, analyzer,
   retraction, target and semantic validation; header-only discovery is not a
   replacement for complete closure validation.
2. Derive retained families from that validated closure. Referenced lexical and
   RaBitQ generations must come from their actual canonical names/decoded
   identities, rather than assuming every family equals the content generation.
3. Enumerate candidates through the same captured authority. Retry names remain
   untrusted candidates; re-evaluate the current closure and live pins before
   each removal. A queue must not silently retarget a different root incarnation.
4. Consult pending/exact pin registrations under the shared registry gate while
   retaining publication exclusion. New captures cannot begin during the sweep.
   Existing pending capture may refine to exact pins concurrently; its transfer
   is atomic with respect to registry lookup.
5. Unlink the admitted canonical component relative to that same directory.
   Report native errors, scan errors, exhaustion and cancellation using existing
   retry/budget semantics. No failed discovery authorizes any removal.

A Path may label reports or select initial admission bounds. It cannot choose a
head, candidate namespace, reopened file or deletion target once captured.
Changing only unlink is unsafe: replacement root B can publish generation 6
while captured root A still references generation 1. B's valid retention set
would authorize deleting A's active source, even with perfectly relative unlink
and no missing reader pins. Both immediate and queued cleanup require the same
complete boundary. SourceRootCustody's two discovery controls isolate this case.

## Preserve lazy immutable files and descriptor scaling

Do not implement source-bound reads by permanently opening every checkpoint
artifact. Existing managed immutable files are logical references whose idle
native handles are evictable. Independent segment/stored-term lifetimes must
retain source authority and exact pins without growing the permanent FD count
with the complete manifest or corpus size.

The file abstraction therefore needs captured-source reopen authority in its
logical immutable binding, not a pathname fallback. Its cold-open path must:

- borrow the caller's fresh descriptor inventory and I/O wave;
- open only the captured canonical component, without symlink/reparse traversal;
- bind it to the selected logical immutable identity and perform existing cold
  identity validation under ReadAdmissionControl before any payload access;
- keep a native lease through the complete read, then permit idle eviction.

Preserve managed checkpoint identity checks and cache coalescing. Source-scoped
cache keys must distinguish root incarnation and immutable artifact identity;
an alias-path binding lookup alone is not proof that a captured native child is
that managed object. A warm cache must not substitute a foreign namespace's
object after the original path is replaced. Missing source identity, unsupported
binding or insufficient admission refuses; none permits path-based reopening.

Source-specific bindings must not be inserted into an owning global alias map
that creates a cycle from root to project cache back to root. Logical files and
in-flight reads retain root custody; idle cache entries retain only native
leases/keys and cannot extend a root's external reference count. Retirement
marks the root under the registry gate, closes idle handles/native custody and
releases metadata outside it, then unregisters. The project-wide FD cache keeps
its existing limit and eviction/admission behavior; no independent allowance.

Admit retained authority/binding/cache-key/name/control-block capacity before
allocation. Exact pins outlive all child data and in-flight read leases. An
independent stored-term clone owns only its mutation run, not the entire parent
closure. Returned objects retain neither RuntimePermit nor producer busy slot.
Large or sparse maps and cold/warm overlap need physical-allocation evidence;
conservative fixed credits alone do not establish full allocator/RSS accounting.

## Implemented storage primitive and remaining integration

`File::open_captured_immutable` accepts an existing captured SourceDirectory,
project context, canonical component, a trusted ObjectReference and separate
concrete memory/custody receipts. It does not open a file or validate a payload
until a read scope begins. Cold opens use that directory and preserve the
existing complete managed identity hash, length checks and cancellation hooks.
The project cache distinguishes ordinary managed identities from keys containing
a process-unique directory-capture nonce, component and ObjectReference. The
nonce is not a persisted logical incarnation or an ABA proof.

Keys retain only their admitted metadata; the binding retains directory/caller
custody. Logical clones and pinned views share metadata without allocating
another logical record. A pinned native lease drops before its logical custody.
Later reads borrow fresh operation FD inventory, never the constructor's old
reservation. Source retirement closes idle native handles and deallocates empty
BTree nodes before returning key capacity. Lookup/native-lease handoff shares
checkpoint-validation exclusion with immutable object retirement, so retirement
cannot pass its in-flight check and silently leave a racing read alive.

Concrete receipt types are erased with a unique, private boxed owner. Its only
unsafe operation reconstructs the constructor's exact concrete box, moves the
receipt out (freeing that box) and then destroys the moved receipt. Construction
requires Send + Sync + static RuntimeMemoryPermit types; there are no borrowed
payloads, exposed pointers, clones or weak references to that owner. Combined
with SharedAllocation, shared blocks, metadata boxes and data fields are freed
before their receipts refund capacity. Minimum credits include the concrete
receipt allocation. This is an ownership argument, not a complete allocator/RSS
qualification. The existing project/cache base still needs its own admission.

Native tests cover cold and evicted reads after rename/replacement, warm cache
namespace isolation, independent-binding/concurrent-read coalescing, pin/clone
lifetime and native-close-before-refund, fresh reservation inventory, denied
validation, same-length corruption and foreign/invalid/unadmitted construction.
The cache evicts oldest idle handles by repeated selection without a temporary
key vector; this trades extra comparisons for bounded scratch ownership. No
controlled performance result is claimed.

The storage primitive now distinguishes managed references from captured native
identities. `open_captured_native_immutable` opens one regular child relative to
the captured directory under cancellation and descriptor admission, records the
held file's full native ID and length, then closes that acquisition descriptor.
It does not read payload bytes or manufacture an ObjectReference. Cold reopen
compares the recorded ID and length before exposing a read lease. Its cache keys
include capture ID, canonical name and this distinct identity; managed object
retirement cannot match a native entry. Managed construction and cold reopen
retain the full ObjectReference hash validation and charge for those bytes.

The native variant requires cooperative immutable names and exact pins for its
whole logical-file lifetime. Warm entries intentionally refer to the already
verified held file, rather than consulting a mutable pathname. Native IDs and
length cannot detect same-inode, same-length writes, or ID reuse after deletion.
Consequently this primitive does not discharge logical incarnation/ABA,
no-overwrite publication or format-integrity obligations. Decoders retain their
format/block checks. The SourceRootCustody model abstracts stable artifacts; it
does not prove these filesystem premises. A native metadata check must not be
used to downgrade an existing trusted managed digest contract.

Native tests additionally exercise acquisition cancellation, descriptor denial,
metadata-only capture, original-directory reopening after eviction, same-length
replacement with the old inode still alive, and length drift on the same inode.
These checks provide implementation correspondence, not a complete refinement
or power-loss proof.

Search still needs to supply trusted source identity and custody. Its standalone
artifact manifests do not directly contain managed ObjectReferences for every
file. Integration must retain the selected snapshot's immutable managed bindings
where available and use native identity only for genuinely native artifacts;
never use the replaced path's mutable alias map to manufacture either identity.
Root retirement must invoke retire_cached_children before releasing process
custody. These remain required before turning the namespace regressions green
and enabling ordinary readers.

## Shared directory and cache retirement order

Root/native-only publisher custody now stores one shared captured directory.
Root State admission includes its out-of-line directory payload and shared
control block. Publisher marker admission covers those allocations before a
native-only publisher constructs them. Promotion admits the complete receiving
State before moving native custody, so the allocation is continuously covered;
the temporary overlapping credits are released only after transfer. Allocation
costs change; quotas and validation limits do not.

The directory may be shared with a logical file only while that file's concrete
custody retains its exact artifact pin and thereby the root. Key memory must
not retain that pin. On final logical source destruction the file releases its
directory Arc before its custody pin. The last root then retires idle entries
for that capture before dropping its process-lock guard and directory Arc;
State capacity is refunded afterward. A constructor must keep a borrowed live
pin/root through all failure cleanup; an independently escaping directory Arc
would violate this ownership argument. The current end-to-end test constructs
this bridge explicitly; production Search hydration has not yet adopted it.

The native regression promotes publisher custody, installs a Content pin,
constructs a lazy native file, closes the original pin/capture/publisher and
checks peer exclusion. It reads, forces cache eviction, reopens, and drops the
last logical file. A hook immediately before process unlock checks zero cached
handles and verifies peer exclusion still holds; the after-close hook checks
zero native descriptors, and the ledger returns to its baseline. Removing the
explicit pre-unlock cache retirement makes that exact regression fail (zero
passed/one failed/988 filtered). Restoring it passes the 26 selected root and
reader-admission regressions. This is a private integration fixture, not a
claim that complete Search or GC namespace-replacement tests pass.

`HawDBCapturedFileCustody` separately models parent ownership, a logical file,
an in-flight read, idle cache, process custody, directory and root memory. Its
positive finite check visits 15 distinct states (31 generated, depth 6).
`DropRootOnParent`, `UnlockBeforeCacheClose` and
`RefundBeforeDirectoryClose` independently violate LogicalSourceOwnsCustody,
NativeChildOwnsCustody and DirectoryCapacityOwned. Their configurations are
generated from the positive config by one-constant substitution. The model
assumes successful native close/unlock and correct ownership of every logical
file/read; it does not prove pointer refinement, allocator/RSS accounting,
logical incarnation/ABA, native immutability, GC discovery or power-loss safety.

## Transitional captured cleanup context

Both production sweep policies now borrow `CleanupSource` from their successful
publication lease. The borrowed captured directory chooses enumeration and
unlink targets; a Path remains a legacy discovery label and a test-only scanner
input. The retry queue is preserved and bounded when a source cannot be bound,
with `captured_namespace_unavailable` evidence and no scan/delete attempts.
Cancellation, partial completed deletion counts, live-pin checks and deletion
budgets remain observable. Streaming enumeration preserves the existing full
sweep policy and does not materialize a corpus-sized candidate vector; full
scan-work and native-memory qualification is still required.

Legacy complete closure discovery is fenced before and after its pathname I/O.
The sweep refuses a label that now names a different physical root. This
transitional refusal preserves managed full-hash validation and prevents the
observed foreign-namespace deletion, but it does not establish captured-relative
head/closure reads, logical incarnation or namespace ABA safety. Do not replace
the full closure with a header-only check or treat the fences as final proof.
The two original namespace tests still require captured obsolete reclamation,
so safe refusal cannot make them pass. Ordinary governed serving stays gated.

New native controls bind a cleanup context, replace its pathname inside that
scope, then verify enumeration and unlink still select the original directory.
Another control presents foreign retention to both sweeps: they must refuse,
keep original/replacement bytes intact, and preserve the queued retry candidate.
The context also rejects a directory paired with another root's publication
authority, even inside the same project descriptor domain. The selected
cleanup/root/read-admission collection passes 44 cases. These
controls are narrower than the original complete GC acceptance regressions.

## Native correspondence and acceptance

The namespace regression captures A under publication exclusion, renames it,
publishes a valid unrelated B at the old pathname and adds obsolete evidence to
both directories. It then executes each production discovery/cleanup policy:
A's active payload must remain, A's unreferenced obsolete payload must be
reclaimed, and B's evidence must remain byte-identical. The middle requirement
prevents passing by deferring all cleanup; active preservation rejects a partial
fix that only changes unlink. Restore fixtures on failure and retain red logs.

Also require checksum-valid wrong-family manifests, queued candidates across
replacement, replacement after discovery, independent clones and in-flight
reads, close/eviction/reopen, admission/cancellation failures, and final-owner
reclamation. Preserve existing exact resource bounds and recovery semantics.
Run on supported Unix and native Windows; target Clippy is not native execution.

The finite model assumes immutable artifact identities and cooperative native
custody. It does not prove Rust refinement, logical incarnation persistence,
no-overwrite/name ABA, physical crash durability, full memory/I/O accounting,
writer fairness or Mem cutover. Those original MVCC gates remain required.

## Native unlink and idle handles

Captured unlink removes a name while an already opened reader can keep the old
inode readable. On Windows, legacy `FileDispositionInfo` only marks the file for
deletion until the last handle closes. That does not implement the file-only
unlink event represented in the image engine and can leave obsolete mutation
names visible while an idle immutable-file cache retains a handle.

The Windows backend uses `FileDispositionInfoEx` with `DELETE | POSIX_SEMANTICS`
and closes its relative DELETE-capable handle before acknowledging removal.
Other share-delete handles continue reading their original bytes; a new file
under the same component is a different object. Read-only, unsupported-operation
and descriptor failures propagate without a pathname fallback or closing another
owner's handles. No namespace fsync or power-loss durability follows from unlink
alone. See [Microsoft's disposition contract](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-_file_disposition_information_ex).

The native regression keeps a child open, replaces the original root pathname,
unlinks through the held root and requires immediate name disappearance. It then
recreates the name and checks old-handle/new-open separation and unchanged foreign
bytes. The existing Search child-pin and final-owner reclamation assertions stay
enabled. Native Windows execution is required to qualify this backend change;
a passing Unix test or cross-target Clippy is insufficient.

`HawDBCapturedUnlink` models only this observable unlink boundary: successful
acknowledgment removes the old name, foreign B is untouched, and a held old
handle continues reading its original inode when the name is reused. Delayed
unlink, path-based unlink and rebinding a held handle each violate a separate
invariant. The caller's publication/retention proof is a precondition; this model
does not prove GC admission, native syscall behavior, scratch isolation or
power-loss durability. The unchanged image-engine unlink already removes the
namespace entry independently of retained inode data.

# Publication exclusion through an original captured Consumer root

`SearchProjectionPublishLease::acquire_captured` accepts an already captured
native root plus the caller's existing BuildMemory and RuntimeTaskContext. It
never resolves a saved root label or creates a default task/governor. Existing
legacy path constructors delegate after their initial capture and keep their
additional path-binding fence. This supplies the native exclusion entry needed
by stationary producers; it does not yet integrate their target-root birth,
immutable serving view, selector or activation receipt.

## Native correspondence

PublicationRegistration acquires the process-local publication marker and either
borrows an existing same-domain root's native process custody or owns the original
capture/process lock. The lease opens and locks the publication component through
that capture, then verifies the held file against the component in the same root.
Failure drops native custody before process registration can retire. The caller
retains its enclosing source/target recovery owner; this consumed duplicate is
not a recovery constructor and cannot reconstruct authority from a path.

`SourceDirectory::matches_child_file` requires the original project context and
validates one component. Unix uses no-follow fstatat plus held-file metadata,
without opening/closing a second descriptor for a process-owned lock inode.
Windows opens a non-reparse attributes-only validation handle relative to the
held root, charges it to the original descriptor context, and compares the full
volume/128-bit file identity. Thus Windows needs one transient validation slot;
it may refuse under that real cap instead of borrowing an unaccounted domain.
The binding fence neither excludes arbitrary future mutation nor proves logical
incarnation, allocation/birth provenance, source freshness or publication receipt.
Cooperating root exclusion and the higher-level complete binding remain required.

Cancellation and account admission use the supplied task/accounts before native
lock creation. The existing root registry retains its native-close-before-marker
ordering. Captured directories retain the project domain without keeping an old
operation inventory; operation_context borrows that domain's currently admitted
inventory. No new descriptor domain is introduced for validation.

## Evidence and boundaries

The native selection passed 33 cases spanning publication leases and root
registration, including three new cases for label replacement with the original
capture, original descriptor refusal/fresh retry, and original cancellation/tiny
memory refusal before lock effects. Three storage cases passed for replaced files,
missing/invalid components, no-follow symlinks, original-root label replacement,
and comparison of a lock at the exact two-descriptor Unix cap.

`HawDBCapturedPublicationLease` explores **34 generated / 20 distinct states**,
depth **6**. All six Bazel targets executed successfully. Controls require:

| Control | Required violation |
| --- | --- |
| ReopenLabel | OriginalRoot |
| SkipBinding | LockEntryStillBound |
| DefaultAccount | OriginalAccount |
| EarlyClose | ActualNativeExclusion |
| SkipNativeLock | ActualNativeExclusion |

The model assumes the supplied capture/accounts are the original admitted inputs.
It abstracts native OS locking and identity primitives and permits label/control
replacement before the binding fence. It assumes cooperating exclusion afterward;
it is not a perpetual namespace ABA proof. The root-process custody model checks
retained readers, process exclusion and close ordering independently. Finite state
exploration does not prove liveness, physical resource limits, native Windows
behavior, full generation publication or MVCC. Cross-target Clippy and CI results
must be recorded separately; native macOS results are not Windows execution.

This stage adds no source-ledger state, persisted selector or serving-generation
codec. The legacy resident/relocating facade remains until the stationary adapter
and complete C..E closure are implemented. Mem stable/GA exclusion and default
durability remain unchanged.

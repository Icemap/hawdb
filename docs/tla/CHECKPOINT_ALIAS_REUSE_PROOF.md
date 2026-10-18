# Verified checkpoint alias reuse

`ImmutableFileHandles::mount` mounts checkpoint objects while reconstructing a
branch's runtime. This note justifies a Unix optimization of that existing
operation. It does not establish Consumer activation, source-relative Search
hydration, logical incarnation non-reuse, or whole-engine MVCC correctness.

## Authority and assumptions

The durable branch head supplies the trusted `ObjectReference`, including kind,
format version, byte length and digest. The caller owns branch admission and
excludes cooperating mutations of the alias during mount. Managed objects are
immutable while reachable; retirement obeys its existing reachability barrier.
These are also requirements of the original path-based hard-link installation.
This optimization does not protect against an adversary replacing paths or
writing managed objects concurrently with verification.

Let `O` be the open managed-object file, `R` its trusted reference, and `A` the
alias. `Verified(O,R)` means the complete identity-prefixed content digest and
length have matched `R`. Let `id` be the native device/inode identity, and
`regular_lstat(A)` mean that no-follow metadata identifies a regular file.
The reuse predicate is:

```
Verified(O, R) and regular_lstat(A) and id(A) = id(O)
```

Verification runs before this predicate, using the existing fixed 64 KiB buffer
and transient descriptor. `O` remains open through the identity comparison and
binding installation. On Unix this prevents its inode from being recycled during
the comparison. Neither equal bytes, matching length, a link count greater than
one, nor a previous binding-map entry is an identity witness. A symlink to `O`
fails the regular-file condition even when following it would produce `id(O)`.
Windows continues to use the existing installation path; no truncated Windows
file identifier or extra native alias handle is introduced.

## Safety argument

Consider each result of a mount attempt under the assumptions above:

1. Verification fails. The operation returns that error before alias reuse or
   installation. Same-inode, same-length corruption cannot bypass the digest.
2. Verification succeeds and the reuse predicate holds. The alias already names
   exactly the verified bytes. Installing its binding restores the same
   copy-on-write authority as a fresh mount, including after an earlier runtime
   discarded its in-memory bindings. Binding failure is returned to the caller.
   No persistent namespace change is required.
3. Verification succeeds but the predicate does not hold, or optional alias
   metadata lookup fails. The existing candidate hard-link and atomic rename
   install the checkpoint object, followed by binding installation. A detached
   mutable inode, equal-content copy, missing alias or symlink follows this case.

After successful mount, an ordinary mutator encounters the binding and detaches
before writing or truncating. Existing readers keep their original inode. Thus
the optimization preserves the old mount postcondition and the original
copy-on-write induction: a private write cannot change checkpoint bytes, sibling
aliases, or readers that already hold the previous object.

Aliases remain reconstructible from the durable head. Reuse adds no publication,
acknowledgment, WAL boundary or durability relaxation. A crash during reuse leaves
the same alias that was already present; the next admission verifies it again.
The fallback's existing recovery and synchronization obligations remain intact.

## Native evidence and limits

The `immutable_files::tests` regressions check:

- A first installation reaches the actual hard-link/rename observation points;
  the subsequent verified reuse reaches neither. Full native-history auditing
  uses `PowerLossModel::capture`, rather than a mock mount implementation.
- Clearing the binding before reuse still restores copy-on-write. Writing that
  alias preserves the managed object and an existing reader. Remounting after
  that private write restores checkpoint bytes and preserves the private reader.
- An equal-content copy with another hard link is replaced by the managed
  object's inode. A symlink to the correct object is replaced by a regular alias.
- Same-length in-place object corruption is rejected with a digest mismatch,
  despite the alias retaining the same native identity.
- Repeated mounts of 32 aliases release all descriptors and reservations after
  every round, keep the immutable cache cold, and do not raise the descriptor
  high-water mark established by the initial mounts.

The repeated-mount workload emits elapsed time for 256 mounts of 128 KiB objects.
Its paired measurements can isolate this initialization cost; they do not prove
end-to-end query latency, fuzz stability, RSS bounds or large-workload writer
fairness. This is a conditional native refinement argument, not a new TLC run
or a model of filesystem/hardware durability. Source-qualified test logs and
negative controls must accompany claims about a particular implementation.

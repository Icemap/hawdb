# Persistent file I/O configuration

Issue Number: ref #1048

Status: design proposal. No Direct backend or public configuration API is
implemented by this document. Buffered I/O remains the only implemented mode.
The requested delivery includes every persistent regular-file class, including
WAL and metadata; this scope must not be reduced to data files without an
explicit decision.

## Current implementation

At main `b3f00e6d7a7e55e8ac3435d6f01514c9af70bf44`,
`crates/storage/src/file_io.rs` wraps ordinary `std::fs::OpenOptions` and native
file operations. The source contains no `O_DIRECT`, `F_NOCACHE`, or
`FILE_FLAG_NO_BUFFERING` activation, and `DatabaseConfig` has no I/O-mode
selection. `artifact-mmap` is a separate optional read path, not Direct I/O.

File handles carry project-wide descriptor admission, immutable-object identity
validation and, in qualification builds, physical power-loss tracing. A backend
must preserve these boundaries, including handle cloning, cache eviction,
retained readers and atomic replacement.

## Host configuration contract

Expose a typed Rust setting on `DatabaseConfig`, inherited by the storage open
configuration and the canonical project's file-open context. The proposed names
below are illustrative, not available APIs:

```rust
pub enum PersistentFileIoMode {
    Buffered,
    Direct,
}

pub struct PersistentFileIoConfig {
    pub mode: PersistentFileIoMode,
}
```

`Buffered` remains the default on every platform. `Direct` requests the complete
supported persistent regular-file backend. It must never mean best-effort
Direct, data-files-only Direct, or an automatic fallback to Buffered.

Validate the requested mode and incompatible options before opening, creating,
truncating or recovering authoritative database files. Unsupported platforms,
filesystems, alignment capabilities or unqualified file classes return a clear
error. Capability probing must not mutate an existing database. Conflicting
requests for one canonical project fail before changing its active policy;
branches, deferred runtime admission, snapshots, maintenance and recovery inherit
the same project policy. Changing mode requires closing every live owner and
reopening; do not silently retune existing handles.

Expose typed diagnostics for requested and effective mode, backend and alignment
requirements. Every successfully opened Direct database must report the complete
requested policy; partial activation is an open failure. Memory-only databases
perform no file I/O. Browser WASM retains its existing persistent-open refusal.
No environment variable, helper process or command-line switch becomes a
production control plane.

## Platform mechanisms

| Platform | Required regular-file mechanism | Backend obligations |
| --- | --- | --- |
| Linux | `O_DIRECT` | Discover per-file memory/offset/length requirements, preferably through `STATX_DIOALIGN`; verify effective native flags and actual aligned I/O. Unknown support must not be guessed. |
| macOS | `fcntl(fd, F_NOCACHE, 1)` | Check the return value for each acquired native file; preserve the policy across clones and cache reopen. This cache policy is distinct from a device flush. |
| Windows | `FILE_FLAG_NO_BUFFERING` | Query supported sector alignment, use correctly aligned buffers and offsets/lengths, and preserve descriptor admission and sharing/replace semantics. |

Directory descriptors, namespace operations and locks do not carry ordinary file
payloads. Retain their platform-appropriate handling and synchronization; do not
apply an invalid regular-file flag to a directory to claim universal coverage.
Persistent lock-record payloads remain within the regular-file policy.

All native acquisition paths must use the configured backend, including
immutable cache misses, validation reads, temporary publication candidates,
backup/restore destinations, recovery/quarantine and cloned handles. Audit any
raw native escape hatch. Memory mappings bypass ordinary read/write operations;
reject an incompatible Direct/mapping combination rather than silently mixing
policies.

## File classes and safety boundaries

| File class | Required behavior before Direct can be enabled |
| --- | --- |
| Immutable data, indexes and checkpoint artifacts | Aligned bounded reads/writes, exact format length, digest parity, EOF/short-read handling and independent retained-reader lifetimes. |
| Mutable WAL and sealed WAL generations | Preserve acknowledged recovery prefixes, record framing, append/group-commit ordering, truncation/repair boundaries and synchronization dependencies. |
| Catalog, head, manifest and branch metadata | Preserve atomic replacement and file/parent-directory synchronization, consistent generations, idempotent branch outcomes and fail-closed recovery. |
| Publication candidates, scratch, backup and quarantine files | Correct inherited policy, bounded resource admission, cancellation cleanup and no premature publication or destruction of evidence. |

Direct I/O bypasses an operating-system cache; it does not imply durable media
completion, atomic writes, safe publication or higher performance. Preserve
`SyncOnEveryWrite` as the persistent default. Branch creation, deletion and head
publication keep their existing power-loss-safe barriers in every durability
mode. Relaxed commits retain their documented loss window and complete-WAL flush
to the OS; Direct must not silently weaken or redefine that contract.

### Unaligned writes and logical EOF

The existing encodings include variable-sized metadata and WAL appends. A raw
flag change will make existing I/O invalid on aligned backends. Buffer address,
offset, transfer length and the exact logical file length all need an explicit
backend design.

Do not introduce an unjournaled read-modify-write of a sector containing an
already acknowledged WAL prefix. Rewriting unchanged prefix bytes expands the
physical failure surface even if a successful reopen test preserves the bytes.
Padding, partial-sector flushes, truncate-after-padding and repeated flushes must
not expose fake logical records or erase previously durable bytes after torn or
reordered writes. A physical framing/journal change requires its own explicit
format decision and recovery proof; this proposal does not authorize one.

The implementation must first demonstrate a concrete alignment/publication
protocol for every mutable file class. If it cannot preserve the existing
barriers on a platform/filesystem, refuse that Direct configuration. Do not
silently keep WAL or metadata buffered, waive their tests, or market a partial
backend as full Direct support.

### Bounded resource use

Aligned buffers, overlap copies and in-flight transfers require bounded memory
admission before allocation. Alignment-induced read amplification must be
charged to physical-I/O budgets and exposed separately from logical requested
bytes. Do not allocate an unbounded bounce buffer, duplicate file handles outside
descriptor admission, or hide an uncached read behind unaccounted validation.

## Qualification and release gate

Keep implementation Draft until all of these hold on the final source:

- Registered Linux, macOS and Windows tests verify actual native activation,
  error propagation, unaligned caller buffers and ranges, empty files, exact EOF,
  short/interrupted I/O, cloning, concurrent reads, cache eviction and cleanup.
- Buffered default behavior is unchanged. Direct failures occur before
  authoritative mutation, and canonical-path aliases cannot acquire conflicting
  project policies. Read-only opens perform no recovery writes.
- WAL/schema/data/checkpoint/head/branch/backup tests pass with both modes,
  including reopen and successful/rejected transactions and group commit.
- Fault qualification observes the backend's actual aligned physical operations,
  padding and truncation. It includes lost unsynced writes, torn writes,
  reordering and lost acknowledgments. Previously completed durability barriers
  survive; no partial transaction or uncertain empty-branch recreation appears.
  Process-kill/reopen tests alone are insufficient.
- Memory, physical-I/O and descriptor budget refusals remain strict and leave
  previously committed state intact. Mapped reads cannot bypass the selection.
- Pinned formatting, strict workspace/all-target/all-feature Clippy, installed
  prek hooks and all-files prek pass; affected target configurations include
  minimal-WASM strict Clippy. Run the unchanged mandatory local fuzz selection.
- Measure representative workloads before recommending Direct. Record the exact
  platform/filesystem/storage synchronization assumptions; a successful flag
  operation or a faster microbenchmark is not a durability result.

## References and comparison boundaries

[RocksDB's Direct I/O documentation](https://github.com/facebook/rocksdb/wiki/Direct-IO)
describes the three platform mechanisms and aligned reader/writer wrappers. Its
published options apply to SST I/O and explicitly exclude WAL and MANIFEST I/O.
It is a useful platform reference, not proof that HawDB's requested full-file
scope can safely use the same flags without a new backend.

[SQLite's VFS](https://www.sqlite.org/vfs.html) separates host file selection and
operations from the engine. The requested similarity is an explicit embedded
configuration boundary, not an assertion that SQLite exposes a universal
`O_DIRECT` pragma.

- [Linux open and O_DIRECT](https://man7.org/linux/man-pages/man2/open.2.html)
- [Linux statx alignment information](https://man7.org/linux/man-pages/man2/statx.2.html)
- [Apple fcntl and F_NOCACHE](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/fcntl.2.html)
- [Windows file buffering requirements](https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering)
- [Windows FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)

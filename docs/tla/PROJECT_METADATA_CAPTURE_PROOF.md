# Captured project metadata reads

## Native behavior and authority

`ProjectMetadata::from_files` captures one counted project directory before any
selector or catalog read. It reads `manifest.hawdb` relative to that capture and
opens `branches` through the same root, then reads its catalog through the held
child directory. Catalog bounds and decoding apply to the actual opened file;
pathname metadata cannot stand in for that file. Existing path-based catalog
callers share this same bounded file decoder.

A metadata snapshot retains `Arc<SourceDirectory>`. `refresh` shares that root
and performs fresh relative reads, then rejects a changed project selector. It
does not reacquire a root from `ProjectFileDescriptors::root`, accept an equal
UUID as physical authority, acquire a branch writer lock or open the data WAL.
It may observe a newly atomically published catalog within its actual root.
The initial locator still chooses which directory is captured; this is not a
proof of every facade open/admission transition or all project mutations.

The physical project root stays pinned until the last snapshot or transferred
capture drops.
Independent opens each pay for one directory FD. A refresh shares its existing
root; opening the branches directory and catalog requires two transient FDs at
peak. The two-snapshot/busy-main fixture therefore needs six descriptors: two
for the main owner, two independently retained metadata roots, and two transient
catalog-open descriptors. It explicitly checks that peak, retains corrupt WAL
bytes without reading them, and verifies no runtime or head is created. Separate
four-FD tests retain actual exhaustion, unchanged files, refunds and retry. There
is no separate descriptor domain, uncounted handle or pathname fallback.

Pending-child completion keeps a four-slot base live: project capture (one),
child directory/ownership-lock pair (two), and catalog metadata lock (one).
The catalog reader closes before its candidate is published. Unix closes the
candidate writer before one charged rename operation, giving a five-slot peak.
Windows additionally retains a write-capable candidate through the standard
library rename and subsequent `FlushFileBuffers`: candidate (one) plus rename
admission (one), giving a six-slot peak. The parent namespace barrier follows;
neither original ownership nor candidate custody is dropped to fit a limit.
These are managed charged-slot bounds, not an exact count of all OS handles.

Native CI at `d3f914c` (merge `2a3b5580`, tree `4320add1`) confirmed both Windows
five-slot failures as `requested 1, available 0, limit 5`; Linux/macOS completed
at five. That merge adds main's JSON_VALUE work; its storage durability and
recovery fixture paths are unchanged from `d3f914c`. Earlier five-slot native
Windows outcomes remain adverse evidence.

`windows_replacement_policy_funds_held_candidate_and_rename_together` executes
the existing Windows policy function, enabled for unit tests on other hosts.
With four real managed holders, five refuses before rename and preserves both
names/bytes; six replaces while all old readers still read their old bytes.
This checks the actual charging/holding implementation without duplicating it.
It does not establish Windows syscall or power-loss semantics from Unix runs;
that same regression and the facade cases run in native Windows CI.

The facade pair requires every budget from four up to the platform peak to
retain unchanged catalog/head evidence through retry, then requires completion
of the same receipt at five on Unix or six on Windows, with inherited data.
Aggregate recovery limits use the same platform funding. Initial five-slot
Windows refusal is not counted as a pass. Native CI 39167950514 for head
`3261af9f` subsequently passed the held-publication policy and recovery-boundary
selections on all three platforms, confirming six-slot Windows completion.
Its separate Windows directory-move groups still failed three cases each.
The persistent default and all synchronization barriers are unchanged.

## Safety argument and finite check

[HawDBProjectMetadataCapture](HawDBProjectMetadataCapture.tla) has two physical
roots whose selector UUIDs may be identical. Namespace replacement is possible
before or between either read and before refresh. A selected capture is root 1;
root 2 is the replacement. The model covers one initial read and one refresh.

With all three faults disabled, `held = 1` holds initially, namespace replacement
cannot change it and refresh preserves it. Each read assigns its physical source
from `held`. Thus `CaptureRetained`, `SelectorFromCapture` and
`CatalogFromCapture` are inductive. `ready` is reached only after both reads, so
`CompleteSameRoot` follows. This is physical-root agreement, not atomicity across
concurrent logical catalog transactions or integrity of decoded contents.

Registered TLC positive: **17 generated / 12 distinct states, depth 7**, empty
remaining queue. Three independent generated configurations execute the intended
violations:

| Control | Invariant |
| --- | --- |
| PathForSelector | SelectorFromCapture |
| PathForCatalog | CatalogFromCapture |
| RecaptureOnRefresh | CaptureRetained |

All four Bazel targets executed successfully. Input/config/tool/log evidence is
retained; generated controls derive from the base configuration rather than
adding copied mutant files. The checked module hash matches this source.

## Native correspondence and limits

The native replacement regression moves the original project after capture but
before its first child open, then creates a replacement with the same selector
UUIDs and a different valid catalog revision. Initial admission and refresh must
read the original catalog; later removal of the replacement selector cannot
break refresh. Changing the original selector must still fail closed. This
boundary moves with no live child handle, including on native Windows. The
resource regression exhausts catalog-open capacity in the original domain,
requires exact typed refusal and unchanged bytes, then refunds and retries.

Compiled negative-control and final native/platform results are recorded in the
PR, separately from the finite model. The model assumes a valid retained native
capture and says nothing about native syscall correctness, FD admission, memory
or I/O costs, torn/reordered writes, ledger bootstrap, source identity barriers,
branch lifetime or reclamation. Those obligations require their actual evidence.

This adopts captured reads in the real metadata API. It does not hand project
custody through every later branch-admission path, migrate catalog writers or
GraphStore data I/O, implement the Consumer allocator/birth/debt journal, or
activate stationary Consumer serving. Those remain required by the
[stationary implementation contract](../specs/CONSUMER_STATIONARY_PUBLICATION_SPEC.md).

## Deferred facade handoff

The facade now transfers its original root into a catalog-free capture retained
by deferred admission. See [the handoff argument](DEFERRED_PROJECT_CAPTURE_PROOF.md)
for its separate model, slot cost, native regressions and remaining pathname
race. The metadata model above continues to cover metadata reads only.

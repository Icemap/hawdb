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

The physical project root stays pinned until the last metadata snapshot drops.
Independent opens each pay for one directory FD. A refresh shares its existing
root; opening the branches directory and catalog requires two transient FDs at
peak. The two-snapshot/busy-main fixture therefore needs six descriptors: two
for the main owner, two independently retained metadata roots, and two transient
catalog-open descriptors. It explicitly checks that peak, retains corrupt WAL
bytes without reading them, and verifies no runtime or head is created. Separate
four-FD tests retain actual exhaustion, unchanged files, refunds and retry. There
is no separate descriptor domain, uncounted handle or pathname fallback.

Completing a pending child while this project capture remains live has a
five-FD peak: project capture, the child's directory/ownership-lock pair,
catalog metadata lock, and catalog reader or publication candidate. The facade
regression explicitly tests four descriptors refusing completion and retaining
catalog/head evidence through retry, then five completing the same receipt and
preserving inherited data. Aggregate recovery-limit cases use this actual
five-FD requirement; the four-FD metadata-only/read refusal contract remains.
The original failed four-FD completion expectation is retained as adverse
evidence rather than counted as a passing run.

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

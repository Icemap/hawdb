# Current-engine phase collector runtime qualification

Issue Number: ref #987

The qualification-only source is commit
7a7ccfc8656495d750a903a0fc20f3a0342c2ea4 (tree
fd2357f8b061ab9973d031c04482f5cc69dd153b), explicitly bound to engine
310c739dbaa51dd0fab5d61128eda1d2d465c2c3/tree62dc18f4. The committed
source has exactly 19 declared qualification overlays. This is a current
0.10.0 collector wiring qualification, not a final-parent performance campaign.

## Verified execution

The phase-only opt extension SHA-256 is
95ff8d4194dcc0431461f0738642295058f24c29590945fa90fea408ebeb1766.
The complete original consumer remains unchanged, SHA-256
a9275fa36bd57feddc463a53b60f386d37bebba4337b4fee87040ba35ae3a9d1.
Native binaries and frozen import packages are retained locally; their hashes
and qualification records are published here.

Six fresh mode processes cover memory/file × owned/retained/Arrow at 1000 rows,
each with 31 measured samples and a discarded warmup: 186 measured observations,
including 62 attributed owned observations, plus 6 warmups. Every observation
preserves the original complete checksum/value/ownership checks. Owned native
callbacks occur exactly once; retained/Arrow preparation and pull callbacks
are positive. No missing callback or zero counter is interpreted as free work.
The import control observes the v2 protocol, zero initial counters, no overflow,
and no allocation observer. PyArrow is 26.0.0; Python is 3.11.15.

A fresh error control on both backends executes a real malformed Cypher query,
requires ParseError and exactly one positive owned callback, then runs the
unchanged complete healthy owned consumer. The healthy call again records
exactly one callback with valid attribution, proving error unwinding does not
leave native timing depth active. These are two exception controls, not the
complete final exception matrix or timing-ratio observations.

Pinned strict native/minimal-WASM and independent Python Clippy, fmt,
all-files prek/hooks and actionlint 1.7.7 pass. Actual embedded planner execution
preserves every original 110 resource group and adds 10 separate phase groups.
Single-shard input preparation uses 9 controls compatible with the fixed linter;
GitHub currently permits 25 inputs. The initially rejected 15-input linter
attempt remains preserved and is not attributed to a current service limit.

## Limits and remaining acceptance

Owned timing covers the admitted Rust facade and native materialization.
Prepare/pull timing covers those native calls. The boundary residual includes
parameters, ABI/wrappers, host conversion, release and clock overhead; it is not
pure per-value conversion. This build has no allocation observer and does not
supply allocation or whole-process RSS bounds. These observations are not
ordinary-latency comparisons, canonical-storage zero-copy qualification,
statistical tail guarantees or comprehensive speedups.

Parent #986 still requires final composition/source equivalence, the original
90 ordinary/110 resource/default-refusal/explicit-large matrix and complete
allocation/conversion/source-construction/RSS/tail/exception acceptance. Main
has since changed; parent checkpoint-progress failures remain open. This packet
neither waives those failures nor qualifies a future integrated source. All
prior evidence, including the rejected linter preparation, remains preserved.

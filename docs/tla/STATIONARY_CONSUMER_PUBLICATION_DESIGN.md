# Stationary consumer publication: native availability gate

Status: proposed native publication boundary, with a finite availability model
and a native selector-replacement probe. No Consumer selector format, reader
exchange, registry receipt migration or production activation API is implemented
by this gate. The [full acceptance ledger](../specs/MVCC_VERSIONED_PUBLICATION_SPEC.md)
and [installed activation protocol](CONSUMER_INSTALLED_ACTIVATION_DESIGN.md)
remain binding.

## Executable publication without dropping custody

Consumer initialization currently moves its initialized stage into the requested
destination in `Database::create_search_projection_consumer`. Native custody
retains the stage's descendant process/publication locks. Windows Consumer
relocation failed while the five empty-directory exclusive-move cases passed;
the [directory design](CONSUMER_DIRECTORY_RELOCATION_DESIGN.md) records that
source-qualified adverse evidence. Availability of this transition cannot be
assumed from no-replace safety or a Unix pass.

The next activation boundary keeps the consumer root and candidate component
directories stationary. The existing private-component layout already places
unselected C/E output outside the active D view. Complete candidate validation,
immutable-input pins and dependency synchronization precede replacing a small
consumer selector. The selector must choose the E read view and its matching
operation receipt together. Neither moving a directory nor temporarily closing
its process/publication locks is part of selection.

The [bcfa314 Windows matrix](https://github.com/nowledge-co/hawdb/actions/runs/39092591773/job/117572288360)
executes 12 combinations. Standard-library and exclusive moves both succeed
with no retained handle or only the root handle (four successes). Both refuse
with a held child opened relative to the root or by absolute path, with or
without a byte-range lock (eight AccessDenied/5 results). Thus neither a shared
root handle alone nor byte-range locking alone explains the limit. All six
storage cases pass while the same six Consumer relocation cases still fail.
These results belong to that job's recorded PR merge tree, not an assumed head
or an unfiltered current-source qualification.

The native probe `stationary_selector_replacement_keeps_child_custody` holds a
captured directory, a locked descendant, the old selector inode and an old data
file while using the existing `durable_replace_file` path for the small selector.
It checks that the old held selector/data remain readable, new opens select the
new data, a peer cannot acquire the retained lock and all counted descriptors
are released after closure. Both data files and their namespace precede the
selector publication barrier. These are synthetic native operations, not the
Consumer format or a replacement for full recovery verification. Windows must
execute this probe; cross-target compilation is insufficient.

This probe holds the initial namespace stable to isolate availability. It does
not turn path-based `durable_replace_file` into a captured-root publication
primitive. Actual selector installation must rename and synchronize relative to
the same captured source and must not follow a replaced root label. That source
binding refinement and its recorder/lost/torn/reordered-write qualification
remain required.

Initial registration also needs a defined publication boundary. Do not merely
initialize a visible destination in place, remove the stage move, or let ordinary
readers interpret a missing selector as an empty projection. An initial target
reservation must be exclusively owned and durably distinguish incomplete output
from a committed selector. Reopen must refuse incomplete/uncertain initialization;
an acknowledged selector must retain its complete data and receipt roots. The
current facade's initializer-failure cleanup and occupied-destination guarantees
remain required. This initial reservation protocol is not modeled below.

## Finite model and correspondence

`HawDBStationaryConsumerPublication` starts with one private, already-qualified
candidate and retained child custody. Local actions install it, synchronize data,
select it, finish the head barrier and return. It assumes the native environment
permits selector-file replacement while refusing a directory move with held
descendants. The native matrix/probe qualifies those premises separately for
each actual platform and source revision.

The positive model checks six distinct states (six generated, depth six),
including the eventual-return property. It checks unbroken custody, data-before-selection and
acknowledgement-after-head-barrier. It additionally checks eventual return under
weak fairness of the local transition relation. This excludes infinite local
stuttering, but cannot enable an unavailable native operation. Generated controls
change one constant in the positive configuration:

| Control | Required failure |
| --- | --- |
| UseDirectoryMove | Temporal violation: synchronized candidate cannot advance while children remain held. |
| ReleaseCustody | CustodyUnbroken: closing custody is not a valid availability fix. |
| SkipDataSync | DataBeforeSelection. |
| AckBeforeBarrier | AcknowledgementDurable. |

Deadlock reporting is disabled so the directory-move control specifically checks
the temporal requirement on the terminal stuttering behavior. A successful
safety-only run would miss this unavailable transition. The positive eventual
return is conditional on finite, successful local work and native barriers; it
does not establish scheduler/I/O fairness, cancellation latency, failure recovery,
C..E closure, actual allocator/RSS bounds or a size-independent source fence.
Those obligations remain in the installed-activation and custody models and
their native qualification gates. No mechanized Rust refinement is claimed.

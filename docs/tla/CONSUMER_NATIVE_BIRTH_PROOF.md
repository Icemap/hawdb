# Native Consumer generation birth and retained debt

This qualifies the new kernel birth operation, not stationary Consumer serving,
adoption after restart, debt settlement, complete MVCC or Mem activation. The
legacy facade still initializes and relocates a sibling stage. It must not be
silently redirected through this operation until it owns target publication
custody and has the stationary serving/selector protocol.

## Runtime correspondence

`ReservedConsumerAllocation::prepare_stationary_generation` consumes a live,
kernel-created reservation. It captures the supplied original target parent
once using the retained source owner's descriptor domain. It never creates a
governor, loads an allocation token from JSON or reopens a saved target path.
The generation name derives from the distinct project/incarnation/generation/
operation roles. Target and parent locators are bounded diagnostics, not native
adoption authority. The caller excludes competing mutation of its private name;
the eventual serving producer also needs the actual target publication lease.

The source ledger advances `Reserved -> Pending` under the original project
catalog exclusion, after checking the actual UUID branch owner and complete
reservation binding. Bounded marker bytes are prepared before that publication.
No directory effect follows an uncertain intent-publication response. Native
creation uses `CapturedDirectoryCandidate`; a collision never authorizes opening
or deleting the installed entry. The owner retains the original parent, actual
created directory and marker file, including on errors after native effects.

Through that directory it writes the full bound marker with a CRC envelope,
synchronizes the actual marker file, child namespace and original parent entry,
then records `Recorded` through the original source ledger's sealed-candidate
and file/parent barrier. Before exposure, `born_directory` checks that the
installed marker is still the original native file with the expected bounded
bytes and that the source ledger still records this operation as `Recorded`.
A mismatch fails closed without granting producer access or clearing durable
debt. Only a completed response with those checks exposes the original captured
directory to a producer. The operation cannot be retried by guessing whether a
failed syscall took effect. Drop performs no deletion or counter refund; pending
and recorded rows remain retained durable debt, independent of the registry.

The allocation envelope is now `HAWDB_CONSUMER_ALLOCATIONS_V2`, with explicit
birth state. This replaces the development-only baseline without a migration;
older development project databases must be recreated. Project identity remains
V2. Authoritative legacy and Cloud stores are unaffected. All rows remain subject
to the existing 1 MiB / 256-record limits; no settlement is claimed here.

## Bounded checks

`HawDBConsumerNativeBirth` separates intent, exclusive creation, opening, marker
write, file/child/parent synchronization, source receipt, producer access, close
and one crash. A pre-existing foreign directory may already have a matching
marker. Uncovered target writes and namespace entries may be lost independently;
completed barriers remain durable. The source ledger replacement is abstracted
as the previously qualified control-file transaction.

The positive check explores **48 generated / 39 distinct states**, depth **10**.
All four Bazel targets executed and passed, with these intended counterexamples:

| Control | Required violation |
| --- | --- |
| SkipParentBarrier | RecordedBirthSurvives |
| AdoptMarkerOnRecovery | ProducerHasActualBirth |
| ForgetDebtOnClose | NativeDebtRooted |

This is finite safety evidence, not liveness or crash-atomic multi-filesystem
transactions. The model assumes native exclusive-create/private-name custody,
correct source admission and trustworthy completed synchronization. Torn marker
bytes, actual descriptor admission and platform calls require native tests.

Native coverage exercises collision refusal, original-parent label replacement,
full bound marker/checksum, mutated marker refusal, same-byte native marker
replacement refusal on Unix, retained recorded debt after close/reopen,
nonreuse, no registry activation and descriptor refusal before mkdir. Qualification must
record terminal test results separately; this document does not turn a defined
test or a running crash-image campaign into a passing result.

## Remaining implementation

There is no recovery constructor or adoption from marker/path equality.
[Live no-effect settlement](CONSUMER_NO_EFFECT_SETTLEMENT_PROOF.md) now handles
only attempts proven to own no entry; known-born and uncertain attempts remain
retained debt. A lost response can leave a complete
recorded birth but no live producer token. That outcome remains debt; it is not
rollback. The born target can now acquire Search publication exclusion through
its original native directory handle while retaining a borrow of the birth
token. A bounded candidate-output token can copy one file from another captured
directory in the same project descriptor domain into that born target with a
new child name, checks its exact copied bytes against a prior producer-supplied
object reference, and completes file/directory barriers. A same-length change to
the private source fails with born debt retained. Attempted bytes stay charged
after an error; no path or marker content grants target authority.
This is artifact custody only, not a complete C/E closure or selected view.
Explicit resolution with admitted native custody, complete producer closure,
stationary query views, selector/receipt
publication, C..E validation and eventual known-debt reclamation remain required.
Metadata/workspace physical accounting and all-platform lost/torn/reordered
qualification remain independent gates. A private birth never grants readiness,
source freshness, active binding or permission to remove legacy stores.

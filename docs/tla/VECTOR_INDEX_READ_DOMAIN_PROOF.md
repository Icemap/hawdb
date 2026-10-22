# Vector index read-domain refinement

Tracked by https://github.com/nowledge-co/hawdb/issues/1157. This model qualifies
the source-binding extension to Consumer publication. It assumes the native
incarnation custody, complete immutable artifact closure and durable selector
contract described in [the Consumer proof](CONSUMER_INCARNATION_CUSTODY_PROOF.md)
and [the runtime protocol](../CONSUMER_STATIONARY_PUBLICATION.md). It adds no
second publication authority.

The bounded domain has two database identities, two branch identities, three
source epochs, two schema identities and two immutable index generations. A
source stamp abstracts the complete canonical vector-to-row mapping; a change
to data, deletion or a schema changes its identity. Initial closures include
foreign or stale claims, so acceptance cannot rely on a manifest merely being
well-formed. A snapshot can be retained before binding and a bound read can
survive source mutation and successor selection. `Prepare` and `Select` do not
require `Advance`: two generations can bind the same source stamp. This refines
full rebuild acknowledgement at an unchanged source epoch, authorized by the
complete native selected receipt rather than equality with cached old bytes.

| Model action or invariant | Runtime refinement |
| --- | --- |
| `Capture` | `Database::begin_read_transaction` pins canonical data and schema |
| `BindRead` | `DatabaseReadTransaction::bind_vector_index` verifies database, branch, epoch and the resolved descriptor |
| `Prepare` | Full canonical source enumeration, followed by existing `prepare_registered` |
| `Select` | `finish_consumer_successor` revalidates source under the original publication authority |
| `ReadUsesOneDomain` | `VectorIndexReadTransaction` owns its source snapshot and immutable Consumer pin; generation reports use the native reservation number |
| `PublicationUsesCapturedSource` | A completed replacement is selected only for its complete captured epoch |
| `CloseRead` | Canonical and native pins close together; no idle execution slot is retained |

Five independent controls omit identity, epoch, schema or publication checking,
or retarget an existing read to a new generation. Each must violate its named
invariant in `mutants/mutants.txt`. The model covers finite safety; it does not
prove allocation bounds, numerical recall, IO failures, crash barriers or
language semantics. Those remain runtime and integration tests.

Replay the positive model and negative controls:

```bash
bazel test //docs/tla:HawDBVectorIndexReadDomain_check \
  //docs/tla:vector_index_read_controls --test_output=errors
```

On 2026-10-22 the command completed with six passing Bazel targets. The positive
model explored 48,983 distinct states (139,539 generated), depth 10, with an
empty queue and no invariant violation. All five controls produced the expected
named invariant violations. This is finite safety evidence under the assumptions
above; it does not establish the full vector-index delivery contract.

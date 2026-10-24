# Ordinary relational projection admission

Issue Number: ref #1160

Projection and ordering can repeat one borrowed field many times. Admitting
the source row alone therefore cannot bound the resulting owned payload.

The private `query::projection_memory` module previews each expression
occurrence without evaluating scalar functions or copying source values.
The vector-index path retains its existing admission policy through this
shared module. Ordinary projection copies reserve working memory in the same
query ledger before conversion. Explicit batch and blocking-row limits are
checked before their respective payload copies.

Locator ordering admits its sort record separately and keeps projection
after selection. COUNT DISTINCT input values are admitted before conversion.
DISTINCT ordering admits the added keys and rebuilds output
batches at the existing batch limit. It does not enlarge an input batch
silently. Incoming row ownership and temporary ordering copies remain charged
until the output batch owns them. Reservations retire on Stop or error.

Ordered-index output, ordinary streaming output, borrowed full scans and
FROM-less expressions also admit their copies. Borrowed full scans check the
complete result-row payload before conversion. Result limits, batch limits
and working limits remain separate policies; their defaults are unchanged.

The previews are conservative working-memory estimates. They do not evaluate
volatile functions twice, change query results or provide zero-copy output.
No public API, persistent format or durability policy changes.

The existing relational unit lane owns allocation-before-copy and functional
regressions. The allocation probe counts cumulative System requests above a
payload threshold on the current test thread, with an identical narrow control.
Those counts are not live memory, RSS or benchmark latency measurements.

See [the test index](TEST_INDEX.md) for test ownership and replay.

# Mixed-growth point-cache diagnosis

Issue Number: ref [#1001](https://github.com/nowledge-co/hawdb/issues/1001)

This is a cache-behavior experiment on main
`277178caaeda9affd68b8703d67d71aec3e47a0f`, with the benchmark controls in this
change. It does not attribute the historical post-diagnostic latency regression
or qualify the broader #986 performance/resource gate.

Each producer ran 128 fresh in-memory raw-API processes: 1,000/10,000 rows,
stable/rotating point parameters, diagnostics `none`/`explain`/`analyze`/`both`,
and point bindings `full`/`minimal`, over four reordered rounds. The first round
is discarded for descriptive timing summaries. Each process retains the original
individual CREATE workload, default budgets and durability, verifies every
returned point value, and executes 100 calls in each point phase. `minimal`
removes the unused score binding only from reads; inserts remain unchanged.

All 256 processes succeeded. For both sizes and every diagnostic mode:

| Point bindings | Before: first-phase hits/misses | After: first-phase hits/misses | Both: second-phase hits/misses |
| --- | ---: | ---: | ---: |
| Stable, full or minimal | 99/1 | 99/1 | 100/0 |
| Rotating, full | 0/100 | 99/1 | 100/0 |
| Rotating, minimal | 99/1 | 99/1 | 100/0 |

Second-phase eviction counts were zero. Thus the unused binding fragments the
first rotating phase on this source; diagnostics do not invalidate its warmed
second-phase point cache. This evidence cannot explain the historical 8-14%
second-phase regression reported against different frozen producers.

Both binaries use the repository-pinned Rust 1.97.1 bench profile on macOS arm64.
Other host work was present. No latency speedup, process RSS bound, file-backed
result, foreign-language qualification or historical producer A/B claim follows.
The before and after SHA-256 identities are respectively
`f142936f87aefe3de47fbf86e9c13f26034ab93e3f4264a5361cf2c223476a5e` and
`e824d88cab079015de7747b08eff926708b8d86bcacbac8978c04ace8e7c7a3e`.
Complete local JSON journals, manifests and summaries are retained under
`/private/tmp/hawdb-1001-phase-evidence-kea3wd9a` and
`/private/tmp/hawdb-1001-phase-evidence-z51at9lv`. They are local diagnostic
artifacts, not remotely published acceptance receipts.

Reproduce each matrix cell with the same separately frozen before/after producer:

```console
cargo bench --locked --bench mixed_point_growth --no-run
./target/release/deps/mixed_point_growth-<hash> 1000 - raw rotating both full
```

Vary rows, parameter mode, diagnostic mode and binding mode as listed above.
Keep all samples and errors; compare cache counters independently of timings.
The component regressions cover unused-binding omission, combined template
validation, exact LIMIT keys and missing required parameters. The embedded
regression verifies rotating point values across bound diagnostics. Their
ordinary CI results remain distinct from this manual binary experiment.

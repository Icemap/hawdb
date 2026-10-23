# Consumer progress evidence, 2026-10-22

This is an **incomplete qualification with a failed strict performance gate**.
See [the report](../../CONSUMER_PROGRESS_QUALIFICATION.md). Historical passes do
not qualify later source changes.

| Path | Source and disposition |
| --- | --- |
| `strict-ci-39784901600/` | Exact implementation commit `55e6b3873c61b520635d6fe2b55f76a6de1c988d`, original baseline `a703cc0f18f549918c73f454cda521edb4f0fb13`; 480 cases, two passing endpoints and four persistent-write regressions. Build logs, raw samples and all protocol/source hashes retained. |
| `strict/` | Earlier local frozen source, identified by its complete candidate-source manifest; 480 cases, three passing endpoints and three inconclusive durable intervals. Gate failed. |
| `consumer-before-retention/` | Same earlier local source; 13 completed cases and a retention failure. The finite small/eight-writer detached sample also misses the repeated-active-progress criterion. Not a complete timing qualification. |
| `before-retention-source-overlay.tar.gz` | Seven original files over implementation commit `55e6b387`; the reconstruction was checked against all 2,110 original source hashes. Use only to reconstruct the earlier source described by the local manifests. |
| `validation/retention-covered-red.log.gz` | Reproduction of the false old-C refusal with complete retained E before the correction. |
| `validation/retention-final-{unit,coverage,mvcc,prek}.log.gz` | Implementation-source correctness and required lint: 7 stationary cases, 23 coverage cases, 17,160 MVCC cases, formatting and strict workspace Clippy. |
| `validation/retention-final-bazel.log.gz` | Interrupted implementation-source run: 17 cached passes, 81 skipped. Stopped to integrate new main; not a final fuzz pass. |
| `validation/merge-main-{covered,coverage}.log.gz` | Main integration with fresh per-stage descriptor operations: 8 stationary cases and 23 coverage cases. |
| Remaining `validation/` logs and archives | Earlier development validation, before the retained-E correction and main integration. Includes the earlier 98/98 Bazel receipt, 1,877-case MVCC campaign and explicit admission-pressure failures. These are historical evidence only. |
| `summarize-progress.py`, `summarize-writers.py` | Offline aggregation helpers for the still-outstanding complete matrices; no final matrix receipt is implied by their presence. |

The CI binaries are available in the original Actions artifact while retained.
Their hashes and exact source/build protocol are permanent here; binaries are
not checked into the repository. Durable fixtures were disk-backed, not tmpfs.

Read compressed JSON Lines with standard tools:

```sh
gzip -dc strict-ci-39784901600/results.jsonl.gz | python3 -m json.tool --json-lines
sha256sum -c SHA256SUMS
```

The protocol and receipt raw hashes refer to the **decompressed** samples.
`SHA256SUMS` covers the packaged files. Preserve the one labelled warm-up pair,
all measured pairs and serialized controls when recomputing the strict receipt.

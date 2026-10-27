# Durable raw evidence archive

Issue Number: ref #1222

The [evidence-only prerelease](https://github.com/nowledge-co/hawdb/releases/tag/evidence-pr-1240-2026-10-24)
retains all 55 raw files from the historical 720d98f and integrated 9bb71b8 controls:
2,830,882 uncompressed bytes, including adverse results and the interrupted
pre-sample attempt. It is not a software release or `latest`; its tag points to
existing main 80c4b84357757ac6344e90f8fd4b73dbf9bcd0d0.

The [926,719-byte archive](https://github.com/nowledge-co/hawdb/releases/download/evidence-pr-1240-2026-10-24/architecture-pre-admission-9bb71b8.tar.gz)
has SHA-256 `dd743e24a3799028460cf3e35ba7ee3cd51da6b52ed225150c81143f12b4ed0f`.
The [per-file manifest](https://github.com/nowledge-co/hawdb/releases/download/evidence-pr-1240-2026-10-24/pre-admission-manifest.json)
and [asset checksums](https://github.com/nowledge-co/hawdb/releases/download/evidence-pr-1240-2026-10-24/PRE_ADMISSION_SHA256SUMS)
identify every member and source manifest. All three assets were downloaded
before publication and again after publication. Every asset hash, member path,
size and SHA-256 matched the complete source inventory before removing local
source copies. Compact summaries and reproduction information remain in Git.

These measurements predate the descriptor-memory admission correction and do
not qualify the corrected implementation. Later corrected-source evidence must
use a separate immutable asset, with its actual source/binary/harness identity.

Restore original relative paths into a fresh directory, or omit `--destination`
to restore ignored raw paths under the repository root:

```console
python3 docs/benchmarks/architecture_2026_10_24/restore-raw.py --group pre-admission --destination target/architecture-evidence
```

The helper uses Python 3.9+ and `gh`, checks the pinned archive and complete
manifest before writing, and refuses existing destinations or unsafe members.
It copies regular file data without applying archived owners or modes. Restored
raw paths are ignored. This is an ordinary removal commit, with no history
rewrite; historical PR refs can still retain the original objects.

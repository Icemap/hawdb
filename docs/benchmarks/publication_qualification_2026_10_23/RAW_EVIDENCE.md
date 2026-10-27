# Durable raw evidence archive

Issue Number: ref #1119, ref #232

The [evidence-only prerelease](https://github.com/nowledge-co/hawdb/releases/tag/evidence-pr-1217-2026-10-24)
retains all 654 raw files removed from this tree: 368,556,941 bytes from source
`ef38a62742ac705b862e194b25757fb166b53423`. The 577 original summaries, scripts,
hash indexes and small receipts remain in Git (4,084,280 bytes). Failed and
adverse allocations are included without filtering. Release assets do not have
Actions artifact retention expiry; this prerelease is not a software release or
`latest`. Its tag points to existing main `720d98f8640105add1007040ac608df1e643fc3e`.

[Archive workflow 40062320602](https://github.com/nowledge-co/hawdb/actions/runs/40062320602)
bound checkout to that exact source, checked the complete 1,231-file original
inventory, uploaded four assets, then downloaded all four and verified both
asset hashes and every tar member before publishing. See
[the complete raw manifest](raw-manifest.json) and [asset checksums](ARCHIVE_SHA256SUMS).
The downloaded manifest's 654 paths and sizes also match the GitHub source tree.

Restore the original relative paths before running historical verification:

```console
python3 docs/benchmarks/publication_qualification_2026_10_23/restore-raw.py
```

The helper requires Python 3.12+ and `gh`, verifies the complete archive and every
member before extraction, and refuses to overwrite existing files. Restored raw
files are ignored. Existing historical checksum indexes are unchanged.

This is an ordinary deletion commit. The repository's configured squash merge
keeps these removed blobs out of main's ancestry; do not use a merge-commit
override for this PR. Historical PR refs can still retain the original objects:
this is not a claim of server-wide object erasure.

# PR986 final-source control evidence

Issue Number: ref #976, ref #987, ref #1081

Engine `0c2989842321d10e91f136f3c924badb07038add`, tree `122ad7e9bcbe6ccc38e49dd018bed00c4d02c3a3`. `packet.json` indexes raw reports, audit scripts, commands, frozen-source hashes and limitations.

The six allocation controls comprise three default1k/file positives and three explicit-large1M+1/file exact-row refusals. The six phase controls comprise three explicit-large1M/file positives and three1M+1/file exact-row refusals. Each mode has31measured samples plus warmup. Large supplementary fixture construction explicitly checkpoints each65536rows; original matrices and failures remain separate.

The original ten retained controls include six positives, two default memory-budget refusals and two explicit row-limit refusals. Native binaries are identified by hashes and source/build commands rather than added to Git; the original retained producer archives remain available from Actions run39154784609. Absolute paths in original receipts preserve provenance.

No complete90/110 performance acceptance, checkpoint-cause resolution, whole-memory guarantee or independent approval follows.

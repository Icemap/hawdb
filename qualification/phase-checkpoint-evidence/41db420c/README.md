# Supplemental file-result boundary evidence

Issue Number: ref #987, ref #976, ref #1081

Engine: `41db420c5d5caaeeaac640cc66c96bda9a19de8c`, tree `d386d053f89ae7d2955b57da6da39208ddc616c4`.
Observer: `632d16a2f94e4082526e5b626ba960fdc6cc1929`. This packet commit is documentation only and is not a qualified producer source.

The original phase run39133574819 failed its explicit-large1M+1/file fixture before queries. The source-identical local diagnostic reproduces the MVCC payload refusal after762368 committed rows. Preserve this failure: it is not a successful row-limit test.

The supplemental observer explicitly executes the existing CHECKPOINT statement every65536 setup rows. It preserves safe durability, constructor/configuration,512-row mutation batches, full consumer and31samples plus warmup. Each fresh process records15checkpoints before measurement. No engine or native producer changes. The frozen native extension and complete consumer retain their original SHA256; all frozen files are checked before and after each process.

All six fresh file-backed groups completed: owned/retained/Arrow each successfully consumes all1M rows with matching independent checksums, valid phase attribution and release; each mode also rejects1M+1 rows with the exact1M-row budget error in32attempts. Refused attempts have no qualified phase costs. The independent audit recomputes median/p95/max and verifies phase counters, all values and ownership.

Phase attribution measures native work and mixed host/handoff residual, not pure conversion. RSS includes setup, checkpoints, warmup and complete operations; it is not a query-only memory bound. Supplemental explicit-checkpoint timing is not the original fixture's latency and does not replace the original failed matrix. Full ordinary90/resource110 acceptance, historical checkpoint disposition and genuine independent approval remain open.

Required formatting, strict workspace/all-target/all-feature Clippy, all-files prek and automatic commit hooks pass for observer632d16a2. Frozen native Rust inputs are byte-identical to the previously qualified phase producer515a1e5b; no native rebuild was needed. Runtime reports retain their real local environment and absolute paths.

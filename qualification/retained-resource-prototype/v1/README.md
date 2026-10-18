# Retained resource collection prototype

Apply source.patch to exact base 1569a189de70850a5bac154fb8ddaa8d86603325. The patch includes prerequisite/main and Arrow admission fixes, the original resource instrumentation, and the new optional retained-phase-profiling feature. It is a diagnostic source overlay, not a production delivery. manifest.json binds all 38 compiled source inputs and the complete patch. An isolated-index git apply --cached --check passed. No native binary is included.

The original complete numeric consumer is unchanged. Separate phase-only measurements observe native prepare/pull and a boundary residual containing ABI/metadata, wrapper, release and timing overhead. The residual is not pure conversion, and failed queries never qualify as successful latency samples. Allocation-instrumented and phase-only observations remain separate.

Current root and independent Python strict Clippy/format checks pass; the extension was built and runtime callback coverage passed for all six memory/file x owned/retained/Arrow groups. The source-construction probe reports source capacity and constructed logical bytes, not RSS. The additional expected-refusal audit covers three errors on both backends and retained/Arrow, with original errors and full healthy retries preserved. The initial RETURN-only unsupported-plan fixture failed parsing and remains recorded separately.

These receipts refer to a private assembled prototype, not published #986 head30d or standalone Arrow headbfad9. Full original 90/110 matrices and final-source resource/performance acceptance remain open.

This packet is hosted on an evidence-only branch. The hosting commit is not the measured engine source. The exact measured engine is base1569 plus source.patch, bound by the source-input manifest and extension hash. Reports and receipts preserve the actual local paths and adverse fixture outcome. No binaries, raw build-event protocol, environment dumps, credentials, or signed download URLs are included. Original owned/retained/Arrow consumer bytes are included for inspection; the observer is additional qualification instrumentation.

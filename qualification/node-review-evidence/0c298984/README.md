# PR986 Node review evidence

Issue Number: ref #976, ref #987, ref #1081

Engine `0c2989842321d10e91f136f3c924badb07038add`, tree `122ad7e9bcbe6ccc38e49dd018bed00c4d02c3a3`. See `packet.json` for indexed raw logs, command receipts and limitations, and `tracked-source-manifests.json` for runtime/published source identity.

The first complete general failure and the passing isolated/full observations are preserved together. The automatic checkpoint capacity refusal still needs independent disposition; passing runs do not prove its cause. No test deadline, assertion or concurrency was changed. RSS observations start midway through the replay and cannot establish its whole-run peak.

This packet proves neither full performance acceptance nor independent approval. It belongs on an evidence branch and is not added to the PR986 production tree.

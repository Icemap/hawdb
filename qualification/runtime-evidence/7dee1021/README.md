# PR986 integration runtime evidence

Issue Number: ref #987, ref #976, ref #1081

Engine `7dee10212db9d7f465853bb7d46173c188893d06`, tree
`562442cf50a13aac579903847ee6ba295d5d5e4e`, merges actual main1fd242eb.
This frozen packet supports review; it does not establish completed acceptance
or independent approval. PR986 remains Draft.

Two full general runs each passed1783/failed3/ignored1/filtered322 on differing
checkpoint cases. The second run unintentionally repeated full general because
the shell wrapper did not forward Bazel `--test_filter`; the correction receipt
preserves its actual scope. A correct `--test_arg` checkpoint subset passed85,
filtering2024 in209.95s. That subset and later CI passes do not explain or
supersede the full-run failures. Relevant checkpoint/runtime/storage sources
match main and the predecessor; source equality is not a root-cause finding.

Local ordinary Python(300s) and Pydantic(60s) targets timed out. Preserve their
full logs. A one-second live stack sample is an observation after some work,
not a trace at the instant of failure or proof of causality. GitHub Python/C/Go
jobs pass on synthetic mergecef818b090dd4b1f46d21a2192fb51b776d5321e;
its tree is verified equal to the engine tree. Those workflows do not prove
full root/crates Prow qualification. Windows and remaining CI need their own
final evidence.

Contracts33 pass in123.12s under the additional300-second local cap. Formatting,
strict native/C/Python/minimal-WASM Clippy, all-files prek and commit hooks pass.
The post-Python validation receipt and complete available logs record the exact
terminal status of fuzz, optimization build, Go race and portable checks; inspect
that receipt rather than inferring success from a planned command.

The original ordinary90/resource110 and retained10/phase10 campaigns were
rebound and dispatched without changing samples, cases, budgets, concurrency or
deadlines. Their metadata here is a frozen execution snapshot, not completed
performance acceptance. New held-node controls explicitly declare Item(id)
uniqueness in untimed setup. Independent failure disposition, complete resource
and performance evidence, genuine review/maintainer approval, normal Tide
integration and the final raw-blob history exclusion audit remain required.

Every indexed file has a stored-byte hash plus its original-byte hash. Text logs
are JSON-wrapped to preserve original ANSI and whitespace without rendering it.
The30 historical raw benchmark blobs are absent from the production tree and
current main history; they remain reachable on feature history and require the
squash-only final integration and an actual-main history audit afterward.

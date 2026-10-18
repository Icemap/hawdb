"""Additional source-frozen main control. Issue Number: ref #976, ref #987.

The original 3c baseline campaign and old af1/main adverse outcomes remain
separate. This control qualifies neither retained delivery nor total memory.
"""

import hashlib
import itertools
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
ENGINE = "6881bbcc9809a6195d1bab38fb489aa3684c20fc"
BASELINE = "a1012720decca3844864c910360057a5c71dddd8"
BASELINE_TREE = "a0f5fcc0750a3a4c5f35925a16d4d07b481000e6"
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "bindings/benchmarks"))
import compare
import run as driver


def git(repository, *args):
    return subprocess.check_output(["git", *args], cwd=repository, text=True).strip()


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def main():
    output = Path(os.environ["CONTROL_OUTPUT"]).resolve()
    output.mkdir(exist_ok=False)
    evidence = output / "evidence"
    evidence.mkdir()
    size = int(os.environ["BENCH_SIZE"])
    case, layer, backend = [os.environ["BENCH_" + key] for key in ["CASE", "LAYER", "BACKEND"]]
    plan = list(itertools.product([1000, 100000, 1000000], driver.CASES,
                                  ["memory", "file"], ["rust", "python", "go"]))
    index = plan.index((size, case, backend, layer))
    assert len(plan) == 90
    assert git(ROOT, "diff", "--name-only", ENGINE, "HEAD").splitlines() == [
        ".github/workflows/native-bindings.yml", "qualification/integrated_main_final.py"]
    assert not git(ROOT, "status", "--porcelain")
    baseline = output / "baseline"
    subprocess.run(["git", "clone", "--shared", "--no-checkout", str(ROOT), str(baseline)], check=True)
    subprocess.run(["git", "checkout", "--detach", BASELINE], cwd=baseline, check=True)
    assert git(baseline, "rev-parse", "HEAD^{tree}") == BASELINE_TREE
    sources = {"baseline": {"head": BASELINE, "tree": BASELINE_TREE},
               "candidate": {"head": git(ROOT, "rev-parse", "HEAD"), "tree": git(ROOT, "write-tree")}}
    repositories = {"baseline": baseline, "candidate": ROOT}
    # The integrated baseline already has this exact ordinary harness. No source adapter.
    harness = {}
    for relative in compare.HARNESS:
        data = (ROOT / relative).read_bytes()
        assert data == (baseline / relative).read_bytes(), relative
        harness[relative] = hashlib.sha256(data).hexdigest()
    metadata = dict(sources=sources, engine=ENGINE, engine_tree=git(ROOT, "rev-parse", ENGINE + "^{tree}"),
        original_baseline="3cdb2610ba8b984676b80ba22df64821787886d3",
        overlay_files=[".github/workflows/native-bindings.yml", "qualification/integrated_main_final.py"],
        matrix_selection=dict(sizes=[size], cases=[case], layers=[layer], backends=[backend], samples=3),
        discarded_iterations=1, defaults_changed=False, instrumented=False, harness_sha256=harness,
        build="default Bazel configuration, -c opt", group_count=90,
        platform=platform.platform(), machine=platform.machine(), python=sys.version,
        scope="Additional integrated-main control only. Both sides on one runner per group; all 90 selections retained. Original3c baseline and old af1 adverse outcomes remain separate. Shared candidate Go SDK isolates the native libraries. Any partial/refused/failed side prevents a speedup claim. No retained/Arrow or full-memory qualification.")
    save(evidence / "source-qualification.json", metadata)
    producers = {}
    for side, repository in repositories.items():
        compare.require_source(repository, sources[side])
        producers[side] = compare.build_producer(repository, evidence / (side + "-producer"))
        compare.require_source(repository, sources[side])
    # Same shared-Go contract as the historical af1/main control.
    used = {side: {**producer, "go": producers["candidate"]["go"]}
            for side, producer in producers.items()}
    metadata.update(producers=producers, used_producers=used,
                    ordering=["baseline", "candidate"] if index % 2 == 0 else ["candidate", "baseline"])
    save(evidence / "manifest.json", metadata)
    shutil.copyfile(Path(__file__), evidence / "integrated_main_final.py")
    reports, exits = {}, {}
    for side in metadata["ordering"]:
        repository = repositories[side]
        compare.require_source(repository, sources[side])
        for producer in producers.values():
            compare.require_producer(producer)
        command = [sys.executable, "-B", str(repository / "bindings/benchmarks/run.py"),
                   "--output", str(evidence / side), "--sizes", str(size), "--cases", case,
                   "--layers", layer, "--backends", backend, "--samples", "3"]
        for flag in ["rust", "python", "go", "library", "python_extension"]:
            command += ["--" + flag.replace("_", "-"), used[side][flag]["path"]]
        with (evidence / (side + "-driver.log")).open("w") as stream:
            exits[side] = subprocess.run(command, cwd=repository, stdout=stream, stderr=subprocess.STDOUT).returncode
        report = json.loads((evidence / side / "report.json").read_text())
        assert report["source"] == sources[side]
        reports[side] = report
        compare.require_source(repository, sources[side])
        for producer in producers.values():
            compare.require_producer(producer)
    groups = compare.summarize(reports["baseline"], reports["candidate"], {(layer, case, size, backend)})
    metadata.update(groups=groups, driver_exits=exits,
        all_paths_succeeded=all(code == 0 for code in exits.values()) and all(g["qualified"] for g in groups))
    save(evidence / "comparison.json", metadata)
    return 0 if metadata["all_paths_succeeded"] else 1


if __name__ == "__main__":
    sys.exit(main())

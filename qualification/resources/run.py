"""Manual instrumented resource controls. Issue Number: ref #976, ref #987."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "bindings/benchmarks"))
import compare

ENGINE = "254f1cf38990dfbf6f7895b2a09cdc005aa9200a"
TARGETS = (
    "//:hawdb_bench_host_boundary_allocations", "//bindings/benchmarks:python_boundary",
    "//bindings/go/cmd/boundary", "//bindings/ffi:hawdb_ffi_boundary_profile",
    "//bindings/python:_hawdb_boundary_profile",
)


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, text=True).strip()


def identity(root):
    assert not git(root, "diff", "--name-only")
    assert not git(root, "ls-files", "--others", "--exclude-standard")
    return {"head": git(root, "rev-parse", "HEAD"), "tree": git(root, "write-tree")}


def build(root, output):
    source = identity(root)
    output.mkdir()
    with (output / "build.log").open("w") as log:
        subprocess.run(["bazel", "build", "-c", "opt", *TARGETS], cwd=root,
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    suffix = ".dylib" if sys.platform == "darwin" else ".so"
    paths = {
        "rust": root / "bazel-bin/hawdb_bench_host_boundary_allocations",
        "python": root / "bazel-bin/bindings/benchmarks/python_boundary",
        "library": root / ("bazel-bin/bindings/ffi/libhawdb_ffi_boundary_profile" + suffix),
        "python_extension": root / ("bazel-bin/bindings/python/lib_hawdb_boundary_profile" + suffix),
        "go": root / compare.command(["bazel", "cquery", "-c", "opt", "//bindings/go/cmd/boundary", "--output=files"], root),
    }
    producer = {}
    for name, path in paths.items():
        original = path.resolve(strict=True)
        before = compare.sha256(original)
        target = output / original.name
        if name == "python":
            runfiles = compare.freeze_python_launcher(original, target)
        else:
            shutil.copy2(original, target)
        assert compare.sha256(original) == compare.sha256(target) == before
        producer[name] = {"path": str(target), "source": str(original), "sha256": before}
        if name == "python":
            producer[name].update(runfiles=runfiles, runfiles_manifest_sha256=compare.sha256(
                Path(str(target) + ".runfiles_manifest")))
    assert identity(root) == source
    compare.require_producer(producer)
    return source, producer


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sizes", default="1000,100000,1000000")
    parser.add_argument("--cases", default="select,point,fill,fill_bulk,wide")
    parser.add_argument("--layers", default="rust,python,go")
    parser.add_argument("--backends", default="memory,file")
    parser.add_argument("--samples", type=int, default=3)
    args = parser.parse_args()
    output = args.output.resolve(); output.mkdir(exist_ok=False, parents=True)
    assert git(ROOT, "merge-base", ENGINE, "HEAD") == ENGINE
    assert not git(ROOT, "diff", "--name-only", ENGINE, "HEAD", "crates", "benches", "bindings/benchmarks", "bindings/ffi", "bindings/go")
    baseline = output.parent / (output.name + "-baseline-source")
    subprocess.run(["git", "clone", "--shared", "--no-checkout", str(ROOT), str(baseline)], check=True)
    subprocess.run(["git", "checkout", "--detach", compare.BASELINE], cwd=baseline, check=True)
    specification = json.loads((ROOT / "qualification/resources/baseline-source.json").read_text())
    patch = output / "baseline-observer.patch"
    patch.write_text("".join(specification["patch_lines"]))
    assert compare.sha256(patch) == specification["observer_patch_sha256"]
    subprocess.run(["git", "apply", "--check", str(patch)], cwd=baseline, check=True)
    subprocess.run(["git", "apply", str(patch)], cwd=baseline, check=True)
    subprocess.run(["git", "add", "--all"], cwd=baseline, check=True)
    assert identity(baseline) == {"head": compare.BASELINE, "tree": specification["tree"]}
    assert not git(baseline, "diff", "--cached", "--name-only", "--", "src", "crates")
    sources = {}; producers = {}
    for name, root in (("baseline", baseline), ("candidate", ROOT)):
        sources[name], producers[name] = build(root, output / (name + "-producer"))
    manifest = {
        "engine_source": ENGINE, "engine_tree": git(ROOT, "rev-parse", ENGINE + "^{tree}"),
        "sources": sources, "producers": producers,
        "overlay_files": git(ROOT, "diff", "--name-only", ENGINE, "HEAD").splitlines(),
        "baseline_observer_patch_sha256": compare.sha256(patch),
        "matrix_selection": {"sizes": args.sizes, "cases": args.cases, "layers": args.layers,
                             "backends": args.backends, "samples": args.samples},
        "defaults_changed_by_qualification": False, "instrumented": True,
        "scope": ["Native Rust allocation requests, Python traced heap and Go heap remain separate observations.",
                  "Per-child process RSS includes runtime, fixture, setup, warmup and complete operation.",
                  "Direct native phase timers cover ordinary Cypher parameter, engine and result conversion.",
                  "Instrumented timing is not uninstrumented performance qualification or a whole-memory bound.",
                  "The candidate uses the production engine plus an unused untimed typed large-result constructor.",
                  "All refusals, nonzero exits, unsupported groups and absent profiles remain adverse outcomes."],
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    statuses = {}
    for name, root in (("baseline", baseline), ("candidate", ROOT)):
        assert identity(root) == sources[name]
        producer = producers[name]; compare.require_producer(producer)
        command = [sys.executable, "-B", str(root / "bindings/benchmarks/run.py"),
                   "--output", str(output / name), "--sizes", args.sizes, "--cases", args.cases,
                   "--layers", args.layers, "--backends", args.backends, "--samples", str(args.samples),
                   "--native-profiles"]
        for flag, item in producer.items():
            command.extend(["--" + flag.replace("_", "-"), item["path"]])
        with (output / (name + "-driver.log")).open("w") as log:
            statuses[name] = subprocess.run(command, cwd=root, stdout=log, stderr=subprocess.STDOUT).returncode
        assert identity(root) == sources[name]
        compare.require_producer(producer)
        (output / "outcomes.json").write_text(json.dumps(statuses, indent=2) + "\n")
    return 0 if all(status == 0 for status in statuses.values()) else 1


if __name__ == "__main__":
    sys.exit(main())

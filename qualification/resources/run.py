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
compare = None

ENGINE = None
GO_TARGET = "//bindings/go/cmd/boundary:boundary_profile"
CACHE_FLAGS = ["--repo_env=" + key + "=" + os.environ[key] for key in
               ("CARGO_BAZEL_ISOLATED", "CARGO_NET_OFFLINE") if key in os.environ]
TARGETS = (
    "//:hawdb_bench_host_boundary_allocations", "//bindings/benchmarks:python_boundary",
    GO_TARGET, "//bindings/ffi:hawdb_ffi_boundary_profile",
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
        subprocess.run(["bazel", "build", "-c", "opt", *TARGETS, *CACHE_FLAGS], cwd=root,
                       stdout=log, stderr=subprocess.STDOUT, check=True)
    suffix = ".dylib" if sys.platform == "darwin" else ".so"
    paths = {
        "rust": root / "bazel-bin/hawdb_bench_host_boundary_allocations",
        "python": root / "bazel-bin/bindings/benchmarks/python_boundary",
        "library": root / ("bazel-bin/bindings/ffi/libhawdb_ffi_boundary_profile" + suffix),
        "python_extension": root / ("bazel-bin/bindings/python/lib_hawdb_boundary_profile" + suffix),
        "go": root / compare.command(["bazel", "cquery", "-c", "opt", GO_TARGET, "--output=files", *CACHE_FLAGS], root),
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
    global ROOT, ENGINE, compare
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--sizes", default="1000,100000,1000000")
    parser.add_argument("--cases", default="select,point,fill,fill_bulk,wide")
    parser.add_argument("--layers", default="rust,python,go")
    parser.add_argument("--backends", default="memory,file")
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--root", type=Path, default=ROOT)
    parser.add_argument("--engine", required=True)
    parser.add_argument("--baseline-spec", type=Path, required=True)
    parser.add_argument("--baseline-root", type=Path)
    args = parser.parse_args()
    ROOT = args.root.resolve()
    ENGINE = args.engine
    sys.path.insert(0, str(ROOT / "bindings/benchmarks"))
    import compare as comparison
    compare = comparison
    output = args.output.resolve(); output.mkdir(exist_ok=False, parents=True)
    assert git(ROOT, "merge-base", ENGINE, "HEAD") == ENGINE
    candidate = identity(ROOT)
    assert candidate["tree"] == git(ROOT, "rev-parse", "HEAD^{tree}"), "candidate must have no staged source overlay"
    engine_paths = (
        "src", "crates", "benches", "bindings", "Cargo.toml", "Cargo.lock",
        "BUILD.bazel", "MODULE.bazel", "MODULE.bazel.lock", "rust-toolchain.toml",
        ".bazelrc", ".cargo", "build.rs", "tools", "bazel",
    )
    assert not git(ROOT, "diff", "--name-only", ENGINE, "HEAD", "--", *engine_paths), "ordinary qualification must use the exact engine and binding source"
    baseline = args.baseline_root.resolve() if args.baseline_root else output.parent / (output.name + "-baseline-source")
    specification = json.loads(args.baseline_spec.read_text())
    if args.baseline_root is None:
        subprocess.run(["git", "clone", "--shared", "--no-checkout", str(ROOT), str(baseline)], check=True)
        subprocess.run(["git", "checkout", "--detach", compare.BASELINE], cwd=baseline, check=True)
    patch = output / "baseline-observer.patch"
    patch.write_text("".join(specification["patch_lines"]))
    assert compare.sha256(patch) == specification["observer_patch_sha256"]
    if args.baseline_root is None:
        subprocess.run(["git", "apply", "--check", str(patch)], cwd=baseline, check=True)
        subprocess.run(["git", "apply", str(patch)], cwd=baseline, check=True)
        subprocess.run(["git", "add", "--all"], cwd=baseline, check=True)
    assert identity(baseline) == {"head": compare.BASELINE, "tree": specification["tree"]}
    assert not git(baseline, "diff", "--cached", "--name-only", "--", "src", "crates")
    sources = {}; producers = {}
    for name, root in (("baseline", baseline), ("candidate", ROOT)):
        sources[name], producers[name] = build(root, output / (name + "-producer"))
    built_go = dict(producers["baseline"]["go"])
    saved_go = output / "baseline-built-go"
    shutil.copy2(built_go["path"], saved_go)
    assert compare.sha256(saved_go) == built_go["sha256"]
    built_go["path"] = str(saved_go)
    shared_go = producers["candidate"]["go"]
    baseline_go_path = Path(producers["baseline"]["go"]["path"])
    shared_temporary = baseline_go_path.with_name(baseline_go_path.name + ".shared")
    shutil.copy2(shared_go["path"], shared_temporary)
    shared_temporary.replace(baseline_go_path)
    assert compare.sha256(baseline_go_path) == shared_go["sha256"]
    producers["baseline"]["go"] = dict(shared_go, path=str(baseline_go_path))
    manifest = {
        "engine_source": ENGINE, "engine_tree": git(ROOT, "rev-parse", ENGINE + "^{tree}"),
        "sources": sources, "producers": producers,
        "shared_go_source": sources["candidate"], "shared_go_producer": shared_go,
        "independently_built_reference_go": built_go,
        "go_host_profiles": "go" in args.layers.split(","),
        "overlay_files": git(ROOT, "diff", "--name-only", ENGINE, "HEAD").splitlines(),
        "baseline_observer_patch_sha256": compare.sha256(patch),
        "matrix_selection": {"sizes": args.sizes, "cases": args.cases, "layers": args.layers,
                             "backends": args.backends, "samples": args.samples},
        "defaults_changed_by_qualification": False, "instrumented": True,
        "scope": ["Native Rust allocation requests, Python traced heap and Go heap remain separate observations.",
                  "Per-child process RSS includes runtime, fixture, setup, warmup and complete operation.",
                  "Native phases and Go host JSON/copy/free phases remain separate; native phases nest inside complete Go FFI time.",
                  "Instrumented timing is not uninstrumented performance qualification or a whole-memory bound.",
                  "The ordinary candidate uses the exact frozen engine, bindings, workloads and build configuration; only qualification/workflow files may differ.",
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
        if "go" in args.layers.split(","):
            command.append("--go-host-profiles")
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

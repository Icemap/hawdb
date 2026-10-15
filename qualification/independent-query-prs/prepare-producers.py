"""Freeze four opt producers using identical untimed adapters; no PR commits."""

import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / "pr-986-result-accounting-source"
SELECTION = json.loads((ROOT / "selection.json").read_text())
ADAPTER_PATCH = ROOT / "prepared-adapters.patch"
ADAPTER_PATCH.write_text(
    "".join(json.loads((ROOT / "adapters.json").read_text())["patch_lines"])
)
sys.path.insert(0, str(ROOT / "harness/bindings/benchmarks"))
import compare


def command(*args):
    return subprocess.check_output(args, cwd=SOURCE, text=True).strip()


def clean():
    if command("git", "status", "--porcelain"):
        raise RuntimeError("source clone has unrelated changes")


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


records = {}
manifest_path = ROOT / "producers.json"
if manifest_path.exists():
    records = json.loads(manifest_path.read_text())
for name in ["baseline", "1028", "1029", "1036"]:
    if name in records:
        compare.require_producer(records[name]["artifacts"])
        print(name, "frozen receipt already verified", flush=True)
        continue
    clean()
    revision = SELECTION[name]
    subprocess.run(["git", "checkout", "--detach", revision], cwd=SOURCE, check=True)
    base_tree = command("git", "rev-parse", "HEAD^{tree}")
    subprocess.run(
        ["git", "apply", "--check", str(ADAPTER_PATCH)],
        cwd=SOURCE,
        check=True,
    )
    subprocess.run(["git", "apply", str(ADAPTER_PATCH)], cwd=SOURCE, check=True)
    added = []
    for path in sorted((ROOT / "harness").rglob("*")):
        if (
            not path.is_file()
            or path.name == "compare.py"
            or "__pycache__" in path.parts
        ):
            continue
        relative = path.relative_to(ROOT / "harness")
        destination = SOURCE / relative
        if destination.exists():
            raise RuntimeError("unexpected pre-existing harness: " + str(relative))
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, destination)
        added.append(str(relative))
    adapter_paths = [
        "benchmarks.bzl",
        "bindings/ffi/src/lib.rs",
        "bindings/go/ffi.go",
        "bindings/go/hawdb.go",
        "bindings/go/lib_unix.go",
        "bindings/go/lib_windows.go",
    ]
    subprocess.run(["git", "add", "--", *adapter_paths, *added], cwd=SOURCE, check=True)
    tree = command("git", "write-tree")
    patch = ROOT / (name + "-overlay.patch")
    patch.write_bytes(
        subprocess.check_output(
            ["git", "diff", "--cached", "--binary", revision], cwd=SOURCE
        )
    )
    print(name, revision, tree, "building", flush=True)
    artifacts = compare.build_producer(SOURCE, ROOT / (name + "-producer"))
    if (
        command("git", "rev-parse", "HEAD") != revision
        or command("git", "write-tree") != tree
    ):
        raise RuntimeError("source identity changed during build")
    subprocess.run(["git", "diff", "--exit-code"], cwd=SOURCE, check=True)
    compare.require_producer(artifacts)
    records[name] = {
        "head": revision,
        "base_tree": base_tree,
        "measured_tree": tree,
        "overlay_patch_sha256": sha(patch),
        "artifacts": artifacts,
        "harness": {
            str(path.relative_to(ROOT / "harness")): sha(path)
            for path in (ROOT / "harness").rglob("*")
            if path.is_file() and "__pycache__" not in path.parts
        },
        "build": "bazel build -c opt, default config; identical additive ordinary harness and untimed memory constructor only",
    }
    manifest_path.write_text(json.dumps(records, indent=2) + "\n")
    subprocess.run(
        ["git", "apply", "-R", "--index", str(patch)], cwd=SOURCE, check=True
    )
    clean()
    print(name, "frozen and source restored", flush=True)
subprocess.run(
    ["git", "checkout", "extract/986-query-planning"], cwd=SOURCE, check=True
)
clean()
print("ALL_PRODUCERS_FROZEN", flush=True)

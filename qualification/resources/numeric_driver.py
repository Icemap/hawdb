"""Freeze and run owned/retained/Arrow modes in independent processes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
ENGINE = "e33baaea32102e9be9634f7d2630d39ba6f694c5"


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rows", type=int, required=True)
    parser.add_argument("--backend", choices=("memory", "file"), required=True)
    parser.add_argument("--configuration", choices=("default", "large"), required=True)
    parser.add_argument("--samples", type=int, default=31)
    parser.add_argument("--instrumented", action="store_true")
    args = parser.parse_args()
    assert not git("status", "--porcelain")
    assert git("merge-base", ENGINE, "HEAD") == ENGINE
    source = {"head": git("rev-parse", "HEAD"), "tree": git("rev-parse", "HEAD^{tree}")}
    output = args.output.resolve(); output.mkdir(parents=True, exist_ok=False)
    target = "//bindings/python:_hawdb_boundary_profile" if args.instrumented else "//bindings/python:_hawdb"
    with (output / "build.log").open("w") as log:
        subprocess.run(["bazel", "build", "-c", "opt", target], cwd=ROOT, stdout=log, stderr=subprocess.STDOUT, check=True)
    frozen = output / "frozen"
    shutil.copytree(ROOT / "bindings/python/python/hawdb", frozen / "hawdb", ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    suffix = ".dylib" if sys.platform == "darwin" else ".so"
    extension = ROOT / ("bazel-bin/bindings/python/lib" + ("_hawdb_boundary_profile" if args.instrumented else "_hawdb") + suffix)
    shutil.copyfile(extension.resolve(strict=True), frozen / "_hawdb.so")
    for relative in ("bindings/benchmarks/retained_boundary.py", "qualification/resources/numeric.py"):
        shutil.copyfile(ROOT / relative, frozen / Path(relative).name)
    shutil.copyfile(Path(sys.executable).resolve(), frozen / "python-interpreter.bin")
    runtime_hash = digest(Path(sys.executable).resolve())
    files = [p for p in frozen.rglob("*") if p.is_file()]
    before = {str(p): digest(p) for p in files}
    manifest = {"source": source, "engine_source": ENGINE, "engine_tree": git("rev-parse", ENGINE + "^{tree}"),
                "rows": args.rows, "backend": args.backend, "configuration": args.configuration,
                "instrumented": args.instrumented, "samples": args.samples, "producer_hashes": before,
                "interpreter": {"path": str(Path(sys.executable).resolve()), "sha256": runtime_hash, "archived": "frozen/python-interpreter.bin"},
                "overlay_files": git("diff", "--name-only", ENGINE, "HEAD").splitlines(),
                "scope": "Per-mode process RSS, native/foreign observations and31-sample observed tails. Plain and instrumented results are separate; no owned reference can pollute retained/Arrow RSS."}
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    outcomes = []
    for mode in ("owned", "retained", "arrow"):
        assert {str(p): digest(p) for p in files} == before
        assert digest(Path(sys.executable).resolve()) == runtime_hash
        command = [sys.executable, "-B", str(frozen / "numeric.py"), "--package", str(frozen),
                   "--extension", str(frozen / "_hawdb.so"), "--consumer", str(frozen / "retained_boundary.py"),
                   "--rows", str(args.rows), "--backend", args.backend, "--configuration", args.configuration,
                   "--mode", mode, "--samples", str(args.samples)]
        if args.instrumented:
            command.append("--instrumented")
        env = {key: value for key, value in os.environ.items() if key not in ("PYTHONPATH", "PYTHONHOME")}
        env["PYTHONPATH"] = str(frozen)
        with (output / (mode + ".stdout")).open("w") as stdout, (output / (mode + ".stderr")).open("w") as stderr:
            status = subprocess.run(command, cwd=ROOT, env=env, stdout=stdout, stderr=stderr).returncode
        outcomes.append({"mode": mode, "exit": status})
        (output / "outcomes.json").write_text(json.dumps(outcomes, indent=2) + "\n")
        assert {str(p): digest(p) for p in files} == before
        assert digest(Path(sys.executable).resolve()) == runtime_hash
    assert not git("status", "--porcelain")
    return 0 if all(outcome["exit"] == 0 for outcome in outcomes) else 1


if __name__ == "__main__":
    sys.exit(main())

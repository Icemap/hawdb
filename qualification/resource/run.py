"""Explicit numeric resource qualification. Issue Number: ref #986, ref #987."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys


ENGINE = "5d072971d01761f256aa5d0fe06f418e19ab490e"
OVERLAY = {
    ".github/workflows/native-bindings.yml",
    "bindings/python/src/database.rs",
    "bindings/python/src/lib.rs",
    "src/embedded.rs",
    "qualification/resource/consume.py",
    "qualification/resource/observe.py",
    "qualification/resource/observer_probe.py",
    "qualification/resource/run.py",
    "qualification/resource/test_consume.py",
    "qualification/resource/test_observe.py",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--rows", type=int, choices=(1000, 100000, 1000000, 1000001), required=True
    )
    parser.add_argument("--backend", choices=("memory", "file"), required=True)
    parser.add_argument("--preflight-only", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)

    def git(*arguments):
        return subprocess.check_output(["git", *arguments], cwd=root, text=True).strip()

    source = git("show", "-s", "--format=%H%n%T%n%P").splitlines()
    changed = git("diff", "--name-only", ENGINE, "HEAD").splitlines()
    assert set(changed) == OVERLAY, changed
    assert not git("status", "--porcelain")
    source_receipt = {
        "source": source,
        "engine": ENGINE,
        "engine_tree": git("rev-parse", ENGINE + "^{tree}"),
        "overlay_files": changed,
        "rows": args.rows,
        "backend": args.backend,
        "samples": 7,
        "configuration": "explicit_large",
        "production_defaults_changed": False,
        "timed_consumers_changed": False,
        "scope": "Ordinary numeric control and separately instrumented full consumer calls.",
    }
    (output / "source.json").write_text(json.dumps(source_receipt, indent=2) + "\n")
    if args.preflight_only:
        print(json.dumps(source_receipt))
        return

    frozen = output / "frozen"
    packages = {}
    libraries = {
        "ordinary": root / "bazel-bin/bindings/python/python/hawdb/_hawdb.so",
        "instrumented": root
        / "bazel-bin/bindings/python/lib_hawdb_boundary_profile.so",
    }
    for name, library in libraries.items():
        package = frozen / name
        shutil.copytree(
            root / "bindings/python/python/hawdb",
            package / "hawdb",
            ignore=shutil.ignore_patterns("__pycache__", "*.pyc"),
        )
        shutil.copyfile(library, package / "hawdb/_hawdb.so")
        packages[name] = package
    scripts = frozen / "scripts"
    scripts.mkdir()
    consumer = root / "bindings/benchmarks/retained_boundary.py"
    assert consumer.read_bytes() == subprocess.check_output(
        ["git", "show", ENGINE + ":bindings/benchmarks/retained_boundary.py"], cwd=root
    )
    shutil.copyfile(consumer, scripts / consumer.name)
    for name in ("consume.py", "observe.py"):
        shutil.copyfile(root / "qualification/resource" / name, scripts / name)
    interpreter = Path(sys.executable).resolve()
    shutil.copyfile(interpreter, frozen / "python-executable")

    def frozen_identity():
        return {
            str(path): digest(path)
            for path in sorted(frozen.rglob("*"))
            if path.is_file()
        }

    before = frozen_identity()
    interpreter_digest = digest(interpreter)

    def require_frozen_source():
        assert frozen_identity() == before
        assert digest(interpreter) == interpreter_digest
        assert git("show", "-s", "--format=%H%n%T%n%P").splitlines() == source
        assert not git("status", "--porcelain")

    manifest = {
        **source_receipt,
        "producer_hashes": before,
        "interpreter": {"path": str(interpreter), "sha256": interpreter_digest},
        "python": sys.version,
        "platform": platform.platform(),
        "cpu": subprocess.check_output(["lscpu"], text=True),
        "build": "Default Bazel configuration, -c opt; both producers built before measurement.",
        "scope_limits": [
            "Same-engine, explicit-large numeric fixture; not default-policy approval or engine before/after proof.",
            "Original consumer/reference/warmups/seven rotating rounds and complete checksum/release remain.",
            "Instrumented timings are separate from ordinary latency evidence.",
            "Per-call Linux RSS high-water reset covers complete consumption after setup; operational RSS is not an allocation ledger or hard bound.",
            "Python tracing, unchanged Arrow pool statistics and Rust allocator observations overlap; do not sum them.",
            "Rust requested-byte and Arrow pool peaks remain process-cumulative, not reset per-operation peaks.",
            "Interpreter executable bytes are archived; dependent interpreter and Arrow shared libraries are not independently archived.",
            "One-million-plus-one row refusals remain failures with no qualified speedup.",
            "Full heterogeneous, baseline instrumentation, source-reuse, adverse/tail and independent approval gates remain separate.",
        ],
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    with (output / "python-packages.txt").open("w") as log:
        subprocess.run([sys.executable, "-m", "pip", "freeze"], stdout=log, check=True)
    outcomes = []
    for index, producer in enumerate(("ordinary", "instrumented")):
        require_frozen_source()
        command = [
            sys.executable,
            "-B",
            str(scripts / "consume.py"),
            "--receipt",
            str(output / f"{index}-limits.json"),
        ]
        if producer == "instrumented":
            command.extend(("--resources", str(output / f"{index}-resources.jsonl")))
        command.extend(
            (
                str(scripts / consumer.name),
                "--rows",
                str(args.rows),
                "--backend",
                args.backend,
                "--mode",
                "all",
                "--samples",
                "7",
            )
        )
        environment = dict(
            os.environ, PYTHONPATH=str(packages[producer]), PYTHONDONTWRITEBYTECODE="1"
        )
        with (output / f"{index}.stdout").open("w") as stdout, (
            output / f"{index}.stderr"
        ).open("w") as stderr:
            result = subprocess.run(
                command, cwd=root, env=environment, stdout=stdout, stderr=stderr
            )
        outcomes.append(
            {"producer": producer, "command": command, "exit_code": result.returncode}
        )
        (output / "outcomes.json").write_text(json.dumps(outcomes, indent=2) + "\n")
        require_frozen_source()
    raise SystemExit(0 if all(item["exit_code"] == 0 for item in outcomes) else 1)


if __name__ == "__main__":
    main()

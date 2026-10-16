"""Manual unchanged retained controls. Issue Number: ref #978, ref #976, ref #987."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ENGINE = "c6bfb264f1f1ff0b5a4ea099ef0ecb3ac4545c09"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = subprocess.check_output(
        ["git", "show", "-s", "--format=%H%n%T%n%P"], cwd=root, text=True
    ).splitlines()
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", ENGINE, "HEAD"], cwd=root, text=True
    ).splitlines()
    assert set(changed) == {
        ".github/workflows/native-bindings.yml", "qualification/retained/run.py",
        "qualification/retained/configured.py", "src/embedded.rs",
        "bindings/python/src/database.rs", "bindings/python/src/lib.rs"
    }, changed
    subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=root, check=True)
    assert not subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard"], cwd=root
    )
    frozen = output / "frozen"
    shutil.copytree(root / "bindings/python/python/hawdb", frozen / "hawdb")
    extension = root / "bazel-bin/bindings/python/python/hawdb/_hawdb.so"
    shutil.copyfile(extension, frozen / "hawdb/_hawdb.so")
    for name in ("retained_boundary.py", "retained_write_control.py"):
        shutil.copyfile(root / "bindings/benchmarks" / name, frozen / name)
    shutil.copyfile(root / "qualification/retained/configured.py", frozen / "configured.py")
    files = [p for p in frozen.rglob("*") if p.is_file()] + [Path(sys.executable).resolve()]
    before = {str(p): digest(p) for p in files}
    rows, backend = int(os.environ["QUAL_ROWS"]), os.environ["QUAL_BACKEND"]
    manifest = {
        "source": source, "engine_source": ENGINE, "overlay_files": changed,
        "producer_hashes": before, "rows": rows, "backend": backend,
        "configured_limits": {"rows":1000000,"logical_payload_bytes":1073741824,"query_memory_bytes":1073741824,"result_budget_bytes":1073741824,"runtime_memory_bytes":4294967296,"batch_payload_bytes":268435456,"blocking_operator_bytes":67108864},
        "defaults_changed": True, "timed_consumers_changed": False,
        "python": sys.version, "build": "default Bazel configuration, -c opt",
        "scope": [
            "Explicit configured group; defaults are preserved in separate default-source runs.",
            "Python: seven rotating rounds plus original warmup/reference.",
            "No configured Go measurement is claimed.",
            "Batch rows stay at 256. The typed batch byte allowance also raises plan-derived admission; actual batches remain row-limited.",
            "The earlier d56bb8cf campaign retains its 64 MiB owned-admission refusal and missing teardown receipt.",
            "Refused owned references prevent numeric speedup claims.",
            "Process RSS includes setup/runtime; no foreign-heap bound.",
            "The complete ordinary matrix is separate.",
        ],
    }
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    with (output / "python-packages.txt").open("w") as log:
        subprocess.run([sys.executable, "-m", "pip", "freeze"], stdout=log, check=True)
    commands = [[sys.executable, "-B", str(frozen / "configured.py"), str(frozen / "retained_boundary.py"),
                 "--rows", str(rows), "--backend", backend, "--mode", "all",
                 "--samples", "7"]]
    outcomes = []
    for index, command in enumerate(commands):
        assert {str(p): digest(p) for p in files} == before
        env = dict(os.environ, PYTHONPATH=str(frozen))
        with (output / f"{index}.stdout").open("w") as stdout, (output / f"{index}.stderr").open("w") as stderr:
            result = subprocess.run(command, cwd=root, env=env, stdout=stdout, stderr=stderr)
        outcomes.append({"command": command, "exit_code": result.returncode})
        (output / "outcomes.json").write_text(json.dumps(outcomes, indent=2) + "\n")
        assert {str(p): digest(p) for p in files} == before
    subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=root, check=True)
    raise SystemExit(0 if all(x["exit_code"] == 0 for x in outcomes) else 1)


if __name__ == "__main__":
    main()

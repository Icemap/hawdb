"""Manual unchanged retained controls. Issue Number: ref #978, ref #976, ref #987."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    campaign = json.loads((root / "qualification/resources/campaign.json").read_text())
    engine = campaign["engine"]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    source = subprocess.check_output(
        ["git", "show", "-s", "--format=%H%n%T%n%P"], cwd=root, text=True
    ).splitlines()
    changed = subprocess.check_output(
        ["git", "diff", "--name-only", engine, "HEAD"], cwd=root, text=True
    ).splitlines()
    source_check = {
        "status": "checking", "source": source, "engine_source": engine,
        "engine_tree": campaign["engine_tree"], "overlay_files": changed,
        "expected_overlay_files": campaign["overlay_files"],
        "requested": {key: os.environ.get(key) for key in
                      ("QUAL_ROWS", "QUAL_BACKEND", "QUAL_CONFIGURATION")},
        "scope": "Source/request check only; no query or performance outcome.",
    }
    source_path = output / "source-check.json"
    source_path.write_text(json.dumps(source_check, indent=2) + "\n")
    try:
        assert len(engine) == 40 and all(c in "0123456789abcdef" for c in engine)
        assert subprocess.check_output(
            ["git", "rev-parse", engine + "^{tree}"], cwd=root, text=True
        ).strip() == campaign["engine_tree"]
        assert subprocess.check_output(
            ["git", "merge-base", engine, "HEAD"], cwd=root, text=True
        ).strip() == engine
        assert changed == campaign["overlay_files"], changed
        subprocess.run(["git", "diff", "--exit-code", "HEAD"], cwd=root, check=True)
        assert not subprocess.check_output(
            ["git", "ls-files", "--others", "--exclude-standard"], cwd=root
        )
    except Exception as error:
        source_check.update(status="failed", error_type=type(error).__name__, error=str(error))
        source_path.write_text(json.dumps(source_check, indent=2) + "\n")
        raise
    source_check["status"] = "passed"
    source_path.write_text(json.dumps(source_check, indent=2) + "\n")
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
    profile = os.environ["QUAL_CONFIGURATION"]
    assert profile in ("default", "large")
    manifest = {
        "source": source, "engine_source": engine, "overlay_files": changed,
        "producer_hashes": before, "rows": rows, "backend": backend,
        "configured_limits": {"rows":1000000,"logical_payload_bytes":1073741824,"query_memory_bytes":1073741824,"result_budget_bytes":1073741824,"runtime_memory_bytes":2147483648,"batch_payload_bytes":268435456,"blocking_operator_bytes":67108864},
        "defaults_changed": profile == "large", "timed_consumers_changed": False,
        "configuration": profile, "engine_default_result_rows": 0,
        "python": sys.version, "build": "default Bazel configuration, -c opt",
        "scope": [
            "Explicit configured group; defaults are preserved in separate default-source runs.",
            "Python: seven rotating rounds plus original warmup/reference.",
            "No configured Go measurement is claimed.",
            "Batch rows stay at 256. The typed batch byte allowance also raises plan-derived admission; actual batches remain row-limited.",
            "The earlier d56bb8cf campaign retains its 64 MiB owned-admission refusal and missing teardown receipt.",
            "The 6c1779a1 attempt records Linux clamping a requested 4 GiB governor to available memory; no measurements from that failed assertion qualify.",
            "Refused owned references prevent numeric speedup claims.",
            "Process RSS includes setup/runtime; no foreign-heap bound.",
            "The complete ordinary matrix is separate.",
        ],
    }
    if profile == "default":
        manifest["configured_limits"] = {"rows":0,"logical_payload_bytes":67108864,"query_memory_bytes":268435456,"result_budget_bytes":16777216,"runtime_memory_bytes":None,"batch_payload_bytes":16777216,"blocking_operator_bytes":67108864}
        manifest["scope"][0] = "Actual default constructors and zero/unlimited row policy; finite byte budgets remain active."
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

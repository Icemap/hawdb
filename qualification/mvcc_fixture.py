"""Untimed Linux fixture diagnosis. Issue Number: ref #987, ref #976.

No timing ratio, qualification pass or causal claim follows from one pair.
Production and configured-adapter sources are inherited unchanged from 2a1a677a.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def freeze_producer(root, output):
    frozen = output / "frozen"
    shutil.copytree(root / "bindings/python/python/hawdb", frozen / "hawdb",
                    ignore=shutil.ignore_patterns("__pycache__", "*.pyc"))
    shutil.copyfile(root / "bazel-bin/bindings/python/python/hawdb/_hawdb.so",
                    frozen / "hawdb/_hawdb.so")
    env = dict(os.environ, PYTHONPATH=str(frozen), PYTHONNOUSERSITE="1",
               PYTHONDONTWRITEBYTECODE="1")
    probe = json.loads(subprocess.check_output([sys.executable, "-B", "-c",
        "import hawdb,hawdb._hawdb as m,json; assert callable(hawdb.open); "
        "print(json.dumps(dict(extension=m.__file__,package=hawdb.__file__)))"],
        env=env, cwd=output, text=True))
    assert Path(probe["extension"]).resolve() == (frozen / "hawdb/_hawdb.so").resolve()
    assert Path(probe["package"]).resolve() == (frozen / "hawdb/__init__.py").resolve()
    return env, sorted(p for p in frozen.rglob("*") if p.is_file()), probe


def child(mode, output):
    import hawdb
    import pyarrow

    assert pyarrow.__version__ == "26.0.0"

    output.mkdir()
    report = dict(mode=mode, rows_requested=1000000, rows_committed=0,
                  checkpoints=0, started=time.time(), phase="open", pyarrow=pyarrow.__version__)
    db = None
    try:
        db = hawdb.open(output / "db")
        limits = list(db._qualification_limits())
        report["observed_limits"] = limits
        assert limits[:4] == [0, 67108864, 268435456, 16777216]
        assert limits[4] > 0 and limits[5:] == [16777216, 67108864]
        report["phase"] = "seed"
        db.execute("CREATE NODE TABLE Delivery")
        db.execute("CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT")
        for start in range(0, 1000000, 512):
            end = min(start + 512, 1000000)
            db.execute("UNWIND $rows AS row CREATE (:Delivery {score: row.score})",
                       {"rows": [{"score": value} for value in range(start, end)]})
            report["rows_committed"] = end
            if mode == "checkpoint" and end % 65536 == 0:
                report["phase"] = "checkpoint"
                db.execute("CHECKPOINT")
                report["checkpoints"] += 1
                report["phase"] = "seed"
            if end % 65536 == 0:
                (output / "progress.json").write_text(json.dumps(report, indent=2) + "\n")
                print(json.dumps(dict(mode=mode, rows=end)), flush=True)
        if mode == "checkpoint":
            report["phase"] = "checkpoint"
            db.execute("CHECKPOINT")
            report["checkpoints"] += 1
        report["phase"] = "count"
        rows = db.execute("MATCH (n:Delivery) RETURN count(n) AS total").fetchall()
        assert rows == [{"total": 1000000}], rows
        report.update(status="seeded_and_count_verified", count=rows)
    except Exception as error:
        report.update(status="error", error_type=type(error).__name__, error=str(error))
    finally:
        if db is not None:
            try:
                db.close()
            except Exception as error:
                report.update(status="error", close_error=str(error))
    report["ended"] = time.time()
    (output / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report), flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--mode", choices=["original", "checkpoint"])
    args = parser.parse_args()
    if args.mode:
        child(args.mode, args.output)
        return
    root = Path(__file__).resolve().parents[1]
    base = "2a1a677a117a526a06cb2eb5dae224be5ece8624"
    changed = subprocess.check_output(["git", "diff", "--name-only", base, "HEAD"], cwd=root, text=True).splitlines()
    assert set(changed) == {"qualification/mvcc_fixture.py", ".github/workflows/native-bindings.yml"}
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=root).strip()
    output = args.output.resolve()
    output.mkdir(exist_ok=False)
    env, frozen_files, probe = freeze_producer(root, output)
    files = [*frozen_files, Path(__file__), root / "bindings/benchmarks/retained_boundary.py"]
    hashes = {str(p.relative_to(root)) if p.is_relative_to(root) else str(p): digest(p) for p in files}
    manifest = dict(source=subprocess.check_output(["git", "show", "-s", "--format=%H%n%T%n%P"], cwd=root, text=True).splitlines(),
                    adapter_base=base, engine="6881bbcc9809a6195d1bab38fb489aa3684c20fc",
                    overlay_from_adapter=changed, producer_hashes=hashes, import_probe=probe,
                    python=sys.version, scope="Untimed setup diagnosis only; fresh serial processes, unchanged defaults, 1M rows and 512-row mutations. Explicit CHECKPOINT every 65536 rows and after seeding is the sole intervention. Original performance matrix remains independent and incomplete.")
    (output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    for mode in ["original", "checkpoint"]:
        with (output / (mode + ".log")).open("w") as stream:
            result = subprocess.run([sys.executable, "-B", str(Path(__file__)), "--mode", mode, "--output", str(output / mode)],
                                    env=env, cwd=root, stdout=stream, stderr=subprocess.STDOUT)
        assert result.returncode == 0
        assert {k: digest(p) for k, p in zip(hashes, files)} == hashes
    reports = {mode: json.loads((output / mode / "report.json").read_text()) for mode in ["original", "checkpoint"]}
    (output / "summary.json").write_text(json.dumps(dict(reports=reports, performance_qualified=False,
        control_reproduced=reports["original"].get("error", "").startswith("storage error: MVCC version index estimated payload budget exhausted")), indent=2) + "\n")
    for report in reports.values():
        if report["status"] != "seeded_and_count_verified":
            assert report["phase"] == "seed" and report["error_type"] == "StorageError", report
            assert report["error"] == "storage error: MVCC version index estimated payload budget exhausted; release old snapshots and checkpoint before retrying", report


if __name__ == "__main__":
    main()

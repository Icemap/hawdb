"""Repeat adverse Go bulk controls with immutable Linux A/B and A/A producers."""

from pathlib import Path
import hashlib
import json
import os
import re
import statistics
import subprocess
import sys
import zipfile

ROOT = Path(__file__).resolve().parent
OUT = Path(os.environ["RUNNER_TEMP"]) / "go-bulk-diagnostic"
OUT.mkdir(exist_ok=False)
sys.path.insert(0, str(ROOT / "harness/bindings/benchmarks"))
import run as harness

RUN = 38695324983
REVISION = "3e5b014e57e4ee58bc99e790a2d495183b87ae48"
ARTIFACT = "ordinary-fill_bulk-memory-" + REVISION
SELECTION = json.loads((ROOT / "selection.json").read_text())


def sha(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        while block := stream.read(1024 * 1024):
            digest.update(block)
    return digest.hexdigest()


metadata = json.loads(
    subprocess.check_output(
        [
            "gh",
            "api",
            f"repos/nowledge-co/hawdb/actions/runs/{RUN}/artifacts?per_page=100",
        ]
    )
)
artifact = next(item for item in metadata["artifacts"] if item["name"] == ARTIFACT)
assert artifact["workflow_run"]["head_sha"] == REVISION and not artifact["expired"]
archive = OUT / "original.zip"
with archive.open("wb") as stream:
    subprocess.run(
        [
            "gh",
            "api",
            f'repos/nowledge-co/hawdb/actions/artifacts/{artifact["id"]}/zip',
        ],
        stdout=stream,
        check=True,
    )
archive_sha = sha(archive)
assert archive_sha == artifact["digest"].split(":", 1)[1]
producers = {}
with zipfile.ZipFile(archive) as bundle:
    original = json.loads(bundle.read("producers.json"))
    for name in ["baseline", "1028", "1029"]:
        source = original[name]
        assert source["head"] == SELECTION[name]
        for path in ["bindings/benchmarks/run.py", "bindings/benchmarks/checksum.py"]:
            assert sha(ROOT / "harness" / path) == source["harness"][path]
        artifacts = {}
        for kind in ["go", "library"]:
            item = source["artifacts"][kind]
            path = OUT / (name + "-" + Path(item["path"]).name)
            path.write_bytes(bundle.read(name + "-producer/" + Path(item["path"]).name))
            assert sha(path) == item["sha256"]
            path.chmod(0o755)
            artifacts[kind] = {"path": str(path), "sha256": item["sha256"]}
        producers[name] = {"original_source": source, "artifacts": artifacts}
aliases = {}
for alias, name in [
    ("baselineAA", "baseline"),
    ("baselineBB", "baseline"),
    ("pr1028AAAA", "1028"),
    ("pr1028BBBB", "1028"),
    ("pr1029AAAA", "1029"),
    ("pr1029BBBB", "1029"),
]:
    path = OUT / alias
    os.link(producers[name]["artifacts"]["go"]["path"], path)
    aliases[alias] = {"producer": name, "binary": str(path)}
assert len({len(name) for name in aliases}) == 1
report = {
    "terminal": False,
    "original_run": RUN,
    "original_revision": REVISION,
    "original_artifact": artifact,
    "archive_sha256": archive_sha,
    "driver_sha256": sha(Path(__file__)),
    "producers": producers,
    "aliases": aliases,
    "records": [],
    "scope": "Go bulk memory diagnostic at 1k/10k. Exact existing default-opt Linux binaries and FFI libraries from the original matrix; no rebuilding, target flags, budget, durability, fixture or call changes. Six aliases rotate positions for one discarded and eleven measured rounds; each revision has two aliases with identical binary bytes. A/A variance does not establish that adverse A/B observations are noise or qualify the original 986 full gate.",
}


def guard():
    for name, source in producers.items():
        for item in source["artifacts"].values():
            assert sha(Path(item["path"])) == item["sha256"]
    for item in aliases.values():
        assert (
            sha(Path(item["binary"]))
            == producers[item["producer"]]["artifacts"]["go"]["sha256"]
        )


def save():
    temporary = OUT / "report.tmp"
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(OUT / "report.json")


guard()
names = list(aliases)
for rows in [1000, 10000]:
    fixture = harness.fixture(rows, "fill_bulk")
    encoded = json.dumps(fixture, ensure_ascii=False, separators=(",", ":")).encode()
    (OUT / f"fixture-{rows}.json").write_bytes(encoded)
    for iteration in range(12):
        order = names[iteration % len(names) :] + names[: iteration % len(names)]
        for alias in order:
            guard()
            item = aliases[alias]
            directory = OUT / f"{rows}-{iteration}" / alias
            directory.mkdir(parents=True)
            job = {**fixture, "backend": "memory", "path": str(directory / "db")}
            job_path = directory / "job.json"
            job_path.write_text(
                json.dumps(job, ensure_ascii=False, separators=(",", ":"))
            )
            source = producers[item["producer"]]
            command = [
                item["binary"],
                str(job_path),
                source["artifacts"]["library"]["path"],
            ]
            record = harness.run_child(command, directory / "result")
            record.update(
                alias=alias,
                producer=item["producer"],
                layer="go",
                size=rows,
                iteration=iteration,
                discarded=iteration == 0,
                order=order,
                fixture_sha256=hashlib.sha256(encoded).hexdigest(),
                job_sha256=sha(job_path),
                expected_checksum=fixture["expected_checksum"],
            )
            record["parity"] = (
                record.get("status") == "ok"
                and record.get("output_rows") == rows
                and record.get("checksum") == fixture["expected_checksum"]
            )
            record["phase_timing_valid"] = (
                record.get("status") == "ok"
                and all(
                    type(record.get(key)) is int and record[key] >= 0
                    for key in [
                        "query_boundary_ns",
                        "write_boundary_ns",
                        "read_boundary_ns",
                    ]
                )
                and record["query_boundary_ns"]
                == record["write_boundary_ns"] + record["read_boundary_ns"]
            )
            record["profile_valid"] = harness.native_profile_valid(record, "go", False)
            record["qualified"] = bool(harness.qualified_record(record))
            if record.get("status") == "ok":
                assert (
                    record["durability"] == "SyncOnEveryWrite"
                    and record["cgo_enabled"] is False
                )
            raw = Path(record["stderr"]).read_text()
            for field, pattern in [
                ("process_user_seconds", r"User time \(seconds\):\s*([0-9.]+)"),
                ("process_system_seconds", r"System time \(seconds\):\s*([0-9.]+)"),
            ]:
                match = re.search(pattern, raw)
                record[field] = float(match[1]) if match else None
            for stream in ["stdout", "stderr"]:
                record[stream + "_sha256"] = sha(Path(record[stream]))
            report["records"].append(record)
            save()
            guard()
            print(
                rows,
                iteration,
                alias,
                record["status"],
                record["qualified"],
                flush=True,
            )
report["groups"] = []
for rows in [1000, 10000]:
    selected = [r for r in report["records"] if r["size"] == rows]
    group = {
        "rows": rows,
        "qualified": len(selected) == 72 and all(r["qualified"] for r in selected),
        "comparisons": {},
    }
    if group["qualified"]:
        for left, right in [
            ("baselineAA", "pr1028AAAA"),
            ("baselineBB", "pr1028BBBB"),
            ("baselineAA", "pr1029AAAA"),
            ("baselineBB", "pr1029BBBB"),
            ("baselineAA", "baselineBB"),
            ("pr1028AAAA", "pr1028BBBB"),
            ("pr1029AAAA", "pr1029BBBB"),
        ]:
            metrics = {}
            for metric in [
                "query_boundary_ns",
                "write_boundary_ns",
                "read_boundary_ns",
                "elapsed_ns",
                "process_peak_rss_bytes",
            ]:
                pairs = []
                for iteration in range(1, 12):
                    pair = {
                        r["alias"]: r for r in selected if r["iteration"] == iteration
                    }
                    pairs.append(pair[left][metric] / pair[right][metric])
                median = statistics.median(pairs)
                metrics[metric] = {
                    "median_left_over_right": median,
                    "min": min(pairs),
                    "max": max(pairs),
                    "mad": statistics.median(abs(x - median) for x in pairs),
                    "pairs": pairs,
                }
            group["comparisons"][left + "/" + right] = metrics
    report["groups"].append(group)
guard()
report["terminal"] = len(report["records"]) == 144
report["all_paths_succeeded"] = all(r["qualified"] for r in report["records"])
save()
archive.unlink()
print("TERMINAL", len(report["records"]), report["all_paths_succeeded"], flush=True)
raise SystemExit(0 if report["all_paths_succeeded"] else 1)

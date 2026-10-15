"""Rotating paired ordinary controls for three independent PRs; no budget overrides."""

from pathlib import Path
import hashlib
import itertools
import json
import math
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "harness/bindings/benchmarks"))
import run as matrix
import compare

matrix.ROOT = ROOT / "harness"
selection = json.loads((ROOT / "selection.json").read_text())
producers = json.loads((ROOT / "producers.json").read_text())
OUT = ROOT / "ordinary"
OUT.mkdir(exist_ok=True)
manifest_sha = hashlib.sha256((ROOT / "producers.json").read_bytes()).hexdigest()
driver_sha = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
report_path = OUT / "report.json"
if report_path.exists():
    report = json.loads(report_path.read_text())
    assert (
        report["producers_sha256"] == manifest_sha
        and report["driver_sha256"] == driver_sha
    )
else:
    report = {
        "selection": selection["ordinary"],
        "producers_sha256": manifest_sha,
        "driver_sha256": driver_sha,
        "records": [],
        "groups": [],
        "terminal": False,
        "platform": platform.platform(),
        "ordering": "All four producers rotate positions every round. One discarded and seven measured rounds per complete group; shared baseline within each paired round.",
        "limits": [
            "CPU-load observations retained. Same-shard ratios do not establish an absolute SLO.",
            "Ordinary latency only; allocation witnesses are separate binaries.",
            "Fresh independent stores and equal-length producer path components.",
            "Exact original statements, calls, seed, warmup, checksums, default budgets and SyncOnEveryWrite.",
            "Process CPU and RSS include fixture setup and host/runtime; no query-only peak claim.",
            "This 1k/10k independent-PR matrix does not qualify the original #986 100k/1M retained/Arrow gate.",
        ],
    }


def save():
    temporary = report_path.with_suffix(".tmp")
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(report_path)


def cpu_snapshot():
    text = subprocess.check_output(["ps", "-axo", "pid,pcpu"], text=True)
    busy = []
    for line in text.splitlines()[1:]:
        fields = line.split()
        if len(fields) == 2 and float(fields[1]) >= 10:
            busy.append({"pid": int(fields[0]), "cpu_percent": float(fields[1])})
    return {"unix_time": time.time(), "busy_processes": busy}


def artifact_checks():
    for p in producers.values():
        compare.require_producer(p["artifacts"])


artifact_checks()
keys = {
    (r["producer"], r["layer"], r["case"], r["size"], r["backend"], r["iteration"])
    for r in report["records"]
}
assert len(keys) == len(report["records"])
completed = {(g["layer"], g["case"], g["size"], g["backend"]) for g in report["groups"]}
names = ["baseline", "1028", "1029", "1036"]
path_names = {
    "baseline": "baseline00",
    "1028": "pr10280000",
    "1029": "pr10290000",
    "1036": "pr10360000",
}
assert len({len(x) for x in path_names.values()}) == 1
sel = selection["ordinary"]
expected = set(
    itertools.product(sel["layers"], sel["cases"], sel["sizes"], sel["backends"])
)
for count in sel["sizes"]:
    for case in sel["cases"]:
        fixture = matrix.fixture(count, case)
        canonical = OUT / f"fixture-{case}-{count}.json"
        if not canonical.exists():
            canonical.write_text(
                json.dumps(fixture, ensure_ascii=False, separators=(",", ":"))
            )
        fixture_sha = hashlib.sha256(canonical.read_bytes()).hexdigest()
        for backend in sel["backends"]:
            for layer in sel["layers"]:
                group_key = (layer, case, count, backend)
                if group_key in completed:
                    continue
                artifact_checks()
                cpu_before = cpu_snapshot()
                for iteration in range(sel["measured_rounds"] + 1):
                    order = names[iteration % 4 :] + names[: iteration % 4]
                    for name in order:
                        key = (name, layer, case, count, backend, iteration)
                        if key in keys:
                            continue
                        directory = (
                            OUT
                            / f"{layer}-{case}-{count}-{backend}-{iteration}"
                            / path_names[name]
                        )
                        directory.mkdir(parents=True, exist_ok=True)
                        job = {
                            **fixture,
                            "backend": backend,
                            "path": str(directory / "db"),
                        }
                        job_path = directory / "job.json"
                        job_path.write_text(
                            json.dumps(job, ensure_ascii=False, separators=(",", ":"))
                        )
                        artifact = producers[name]["artifacts"]
                        command = [artifact[layer]["path"], str(job_path)]
                        if layer == "go":
                            command.append(artifact["library"]["path"])
                        if layer == "python":
                            command.append(artifact["python_extension"]["path"])
                        result = matrix.run_child(command, directory / "result")
                        result.update(
                            {
                                "producer": name,
                                "layer": layer,
                                "case": case,
                                "size": count,
                                "backend": backend,
                                "iteration": iteration,
                                "discarded": iteration == 0,
                                "order": order,
                                "fixture_sha256": fixture_sha,
                                "job_sha256": hashlib.sha256(
                                    job_path.read_bytes()
                                ).hexdigest(),
                                "expected_checksum": fixture["expected_checksum"],
                            }
                        )
                        result["parity"] = (
                            result.get("status") == "ok"
                            and result.get("output_rows") == count
                            and result.get("checksum") == fixture["expected_checksum"]
                        )
                        result["phase_timing_valid"] = (
                            (
                                all(
                                    type(result.get(k)) is int and result[k] >= 0
                                    for k in [
                                        "query_boundary_ns",
                                        "write_boundary_ns",
                                        "read_boundary_ns",
                                    ]
                                )
                                and result["query_boundary_ns"]
                                == result["write_boundary_ns"]
                                + result["read_boundary_ns"]
                                and (
                                    case in ["fill", "fill_bulk"]
                                    or result["write_boundary_ns"] == 0
                                )
                            )
                            if result.get("status") == "ok"
                            else None
                        )
                        result["profile_valid"] = matrix.native_profile_valid(
                            result, layer, False
                        )
                        result["qualified"] = bool(matrix.qualified_record(result))
                        raw = Path(result["stderr"]).read_text()
                        cpu = re.search(
                            r"([0-9.]+) real\s+([0-9.]+) user\s+([0-9.]+) sys", raw
                        )
                        linux_user = re.search(
                            r"User time \(seconds\):\s*([0-9.]+)", raw
                        )
                        linux_system = re.search(
                            r"System time \(seconds\):\s*([0-9.]+)", raw
                        )
                        result["process_user_seconds"] = (
                            float(cpu[2])
                            if cpu
                            else (float(linux_user[1]) if linux_user else None)
                        )
                        result["process_system_seconds"] = (
                            float(cpu[3])
                            if cpu
                            else (float(linux_system[1]) if linux_system else None)
                        )
                        report["records"].append(result)
                        keys.add(key)
                        save()
                        print(
                            name,
                            layer,
                            case,
                            count,
                            backend,
                            iteration,
                            result["status"],
                            result["parity"],
                            flush=True,
                        )
                        job_path.unlink()
                        if Path(job["path"]).exists():
                            shutil.rmtree(job["path"])
                records = [
                    r
                    for r in report["records"]
                    if (r["layer"], r["case"], r["size"], r["backend"]) == group_key
                ]
                group = {
                    "layer": layer,
                    "case": case,
                    "size": count,
                    "backend": backend,
                    "complete": len(records) == 4 * (sel["measured_rounds"] + 1),
                    "cpu_before": cpu_before,
                    "cpu_after": cpu_snapshot(),
                    "comparisons": {},
                }
                for name in names[1:]:
                    paired = []
                    for iteration in range(1, sel["measured_rounds"] + 1):
                        pair = {
                            r["producer"]: r
                            for r in records
                            if r["iteration"] == iteration
                            and r["producer"] in ["baseline", name]
                        }
                        if len(pair) == 2 and all(
                            r["qualified"] for r in pair.values()
                        ):
                            item = {"iteration": iteration}
                            for metric in [
                                "query_boundary_ns",
                                "read_boundary_ns",
                                "write_boundary_ns",
                                "elapsed_ns",
                                "process_peak_rss_bytes",
                                "process_user_seconds",
                                "process_system_seconds",
                            ]:
                                a, b = pair["baseline"].get(metric), pair[name].get(
                                    metric
                                )
                                if a is not None and b is not None and b > 0:
                                    item[metric] = a / b
                            paired.append(item)
                    comparison = {
                        "successful_pairs": len(paired),
                        "pairs": paired,
                        "all_rounds_succeeded": all(
                            r["qualified"]
                            for r in records
                            if r["producer"] in ["baseline", name]
                        ),
                        "outcomes": {
                            n: [
                                {
                                    "iteration": r["iteration"],
                                    "status": r["status"],
                                    "error": r.get("error"),
                                    "phase": r.get("phase"),
                                }
                                for r in records
                                if r["producer"] == n and not r["qualified"]
                            ]
                            for n in ["baseline", name]
                        },
                    }
                    if (
                        comparison["all_rounds_succeeded"]
                        and len(paired) == sel["measured_rounds"]
                    ):
                        for metric in [
                            "query_boundary_ns",
                            "read_boundary_ns",
                            "write_boundary_ns",
                            "elapsed_ns",
                            "process_peak_rss_bytes",
                            "process_user_seconds",
                            "process_system_seconds",
                        ]:
                            ratios = [p[metric] for p in paired if metric in p]
                            if ratios:
                                median = statistics.median(ratios)
                                comparison[metric] = {
                                    "paired_median_baseline_over_candidate": median,
                                    "min": min(ratios),
                                    "max": max(ratios),
                                    "mad": statistics.median(
                                        abs(r - median) for r in ratios
                                    ),
                                }
                    group["comparisons"][name] = comparison
                report["groups"].append(group)
                completed.add(group_key)
                save()
                artifact_checks()
                print("GROUP", group_key, flush=True)
report["terminal"] = completed == expected and len(report["records"]) == len(
    expected
) * 4 * (sel["measured_rounds"] + 1)
assert report["terminal"], "incomplete requested matrix"
report["all_paths_succeeded"] = all(r["qualified"] for r in report["records"])
report["successful_records"] = sum(r["qualified"] for r in report["records"])
report["failed_or_refused_records"] = (
    len(report["records"]) - report["successful_records"]
)
save()
artifact_checks()
print("TERMINAL", len(report["records"]), report["all_paths_succeeded"], flush=True)

raise SystemExit(0 if report["all_paths_succeeded"] else 1)

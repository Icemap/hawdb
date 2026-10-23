#!/usr/bin/env python3
"""Manual experiment only: identical reads across three projection representations."""
import argparse
import collections
import hashlib
import gzip
import io
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tarfile
import threading
import time

from concurrent_writer_topology import detect

ANCHOR = "823e8e9c735050dcafb3e0bf024e12f0a46ea140"
VARIANTS = ["map", "stack", "vector"]
ORDERS = [VARIANTS, ["stack", "vector", "map"], ["vector", "map", "stack"],
          ["vector", "stack", "map"], ["stack", "map", "vector"], ["map", "vector", "stack"]]
COHORTS = [(False, 1, 2), (False, 8, 16), (True, 1, 2), (True, 8, 16)]
SCHEMA = "concurrent-writers-query-stages-diagnostic-v1"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def prepare(repo, work, output):
    data = repo / "scripts/projection-representation"
    present = subprocess.run(["git", "cat-file", "-e", ANCHOR + "^{commit}"], cwd=repo, stderr=subprocess.DEVNULL)
    if present.returncode:
        subprocess.run(["git", "fetch", "--no-tags", "origin", ANCHOR], cwd=repo, check=True)
    archive = subprocess.check_output(["git", "archive", ANCHOR], cwd=repo)
    sources = {}
    for variant in VARIANTS:
        source = work / variant / "source"
        source.mkdir(parents=True)
        with tarfile.open(fileobj=io.BytesIO(archive)) as stream:
            stream.extractall(source, filter="data")
        patches = ["query-timers.patch"] + ([variant + ".patch"] if variant != "map" else [])
        for name in patches:
            patch = gzip.decompress((data / (name + ".gz")).read_bytes())
            subprocess.run(["git", "apply", "--check"], input=patch, cwd=source, check=True)
            subprocess.run(["git", "apply"], input=patch, cwd=source, check=True)
        for relative, fixture in [("benches/concurrent_writers.rs", "concurrent_writers.rs.txt"),
                                  ("benches/concurrent_writers/readers.rs", "readers.rs.txt")]:
            shutil.copy2(data / fixture, source / relative)
        manifest = {str(path.relative_to(source)): digest(path)
                    for path in sorted(source.rglob("*")) if path.is_file()}
        write(output / f"{variant}-source.json", manifest)
        sources[variant] = source
    for relative in ["Cargo.toml", "Cargo.lock", ".cargo/config.toml", "rust-toolchain.toml",
                     "benches/concurrent_writers.rs", "benches/concurrent_writers/readers.rs"]:
        assert len({digest(source / relative) for source in sources.values()}) == 1, relative
    return sources


def build(source, target, variant, output):
    command = ["cargo", "bench", "--locked", "--bench", "concurrent_writers", "--no-run",
               "--message-format=json", "--jobs", "2"]
    started = time.monotonic()
    with (output / f"{variant}-build.jsonl").open("x") as stdout, (output / f"{variant}-build.log").open("x") as stderr:
        result = subprocess.run(command, cwd=source,
                                env=dict(os.environ, CARGO_TARGET_DIR=str(target)),
                                stdout=stdout, stderr=stderr)
    assert result.returncode == 0, (variant, result.returncode)
    records = [json.loads(line) for line in (output / f"{variant}-build.jsonl").read_text().splitlines()]
    artifacts = [row for row in records if row.get("reason") == "compiler-artifact"
                 and row["target"]["name"] == "concurrent_writers" and row.get("executable")]
    assert len(artifacts) == 1 and not artifacts[0]["fresh"]
    assert artifacts[0]["profile"]["opt_level"] == "3"
    assert not artifacts[0]["profile"]["debug_assertions"]
    binary = output / (variant + "-concurrent-writers")
    shutil.copy2(artifacts[0]["executable"], binary)
    record = {"command": command, "target": str(target), "binary_sha256": digest(binary),
              "seconds": time.monotonic() - started, "profile": artifacts[0]["profile"],
              "features": artifacts[0]["features"]}
    write(output / f"{variant}-build-receipt.json", record)
    print(json.dumps({"built": variant, **record}), flush=True)
    return binary, record


def verify(case, readers):
    assert case["commits"] == case["verified_rows"] == 256
    assert case["reopen_verified"] == case["durable"]
    read = case["read_workload"]
    assert read["pinned_snapshot_verified"] and read["requests"] == readers * 64
    assert all(read[name] > 0 for name in ["reads_during_writer_phase", "writes_during_reader_phase", "intermediate_snapshot_reads"])
    for worker in read["workers"]:
        outer = worker["latency"]["samples_ns"]
        phases = [worker[name]["samples_ns"] for name in ["capture_latency", "query_latency", "drop_latency"]]
        stages = worker["query_stage_samples_ns"]
        assert all(len(values) == 64 for values in phases + [outer, stages])
        assert all(sum(parts) == total for parts, total in zip(zip(*phases), outer))
        assert all(len(row) == 3 and min(row) > 0 and sum(row) <= total
                   for row, total in zip(stages, phases[1]))
    assert case["preflight"]["physical_plan"]


def verify_process(cases, reopen, writers, readers):
    assert len(cases) == 6
    expected = {(False, False, serial) for serial in [False, True]} | {
        (True, grouped, serial) for grouped in [False, True] for serial in [False, True]}
    assert {(row["result"]["durable"], row["result"]["grouped"], row["result"]["serialized_control"]) for row in cases} == expected
    for row in cases:
        assert row["schema"] == SCHEMA and not row["smoke"]
        case = row["result"]
        assert case["writers"] == writers and case["readers"] == readers
        assert case["reopened_before_measure"] == (reopen and case["durable"])
        assert sum(worker["commits"] for worker in case["workers"]) == 256
        assert len(case["workers"]) == writers and all(worker["commits"] > 0 for worker in case["workers"])
        verify(case, readers)


def inventory(output, stop, errors):
    try:
        with (output / "process-inventory.jsonl").open("x") as stream:
            while not stop.is_set():
                names = subprocess.check_output(["ps", "-eo", "comm="], text=True).splitlines()
                busy = [name.strip() for name in names if Path(name.strip()).name in {"cargo", "rustc", "clippy-driver"}]
                stream.write(json.dumps({"unix": time.time(), "compilers": busy}) + "\n")
                stream.flush()
                stop.wait(1)
    except Exception as error:
        errors.append(repr(error))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--fixture-parent", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[1]
    work, fixtures, output = (path.resolve() for path in [args.work, args.fixture_parent, args.output])
    work.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=False)
    protocol = {"anchor": ANCHOR, "diagnostic_ref": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
                "diagnostic_only": True, "variants": VARIANTS, "orders": ORDERS, "cohorts": COHORTS,
                "blocks": 6, "cases_per_process": 6, "measurement_reuse": False,
                "profile": "unaltered repository global bench profile; separate fresh target directories",
                "coverage": "256 writes and 64 reads per reader; full 256-row ID/value/mirror, pins, epochs, overlap and reopen guards",
                "preflight": "One existing full-row preflight uses its identical bounded profile path, then an untimed EXPLAIN records the actual operator shape on all variants.",
                "inventory": "identical 1Hz guest compiler inventory; no physical-exclusive-host claim",
                "python": platform.python_version(), "architecture": platform.machine(),
                "system": platform.system(), "cpu": None if args.prepare_only else detect(),
                "fixtures_sha256": {p.name: digest(p) for p in sorted((repo / "scripts/projection-representation").iterdir()) if p.is_file()}}
    write(output / "protocol.json", protocol)
    sources = prepare(repo, work, output)
    if args.prepare_only:
        return
    fixtures.mkdir(parents=True, exist_ok=False)
    binaries, builds = {}, {}
    for variant, source in sources.items():
        binaries[variant], builds[variant] = build(source, work / variant / "target", variant, output)
    assert len({record["binary_sha256"] for record in builds.values()}) == 3
    assert all(record["profile"] == builds["map"]["profile"] and record["features"] == builds["map"]["features"] for record in builds.values())
    counts = collections.Counter()
    stop = threading.Event()
    monitor_errors = []
    monitor = threading.Thread(target=inventory, args=(output, stop, monitor_errors))
    timing_started = time.time()
    monitor.start()
    measured = {}
    try:
        with (output / "results.jsonl").open("x") as log:
            for reopen, writers, readers in COHORTS:
                for block, order in enumerate(ORDERS):
                    for variant in order:
                        command = [str(binaries[variant]), "--writers", str(writers), "--readers", str(readers),
                                   "--round", str(block % 2), "--fixture-parent", str(fixtures)]
                        if reopen:
                            command.append("--reopen-before-measure")
                        started = time.time()
                        result = subprocess.run(command, capture_output=True, text=True)
                        record = {"reopen": reopen, "writers": writers, "readers": readers, "block": block, "variant": variant,
                                  "command": command, "started_unix": started, "finished_unix": time.time(),
                                  "returncode": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
                        log.write(json.dumps(record) + "\n"); log.flush()
                        assert result.returncode == 0, (variant, result.returncode)
                        cases = [json.loads(line) for line in result.stdout.splitlines()]
                        verify_process(cases, reopen, writers, readers)
                        for row in cases:
                            case = row["result"]
                            counts.update(cases=1, commits=256, reads=readers * 64, reopens=int(case["durable"]))
                            if not case["serialized_control"]:
                                measured[reopen, writers, readers, case["durable"], case["grouped"], block, variant] = case
                        counts["processes"] += 1
                        print(json.dumps({k: record[k] for k in ["reopen", "writers", "readers", "block", "variant", "returncode"]}), flush=True)
    finally:
        timing_finished = time.time()
        stop.set(); monitor.join()
    assert len(measured) == 216
    assert dict(counts) == {"cases": 432, "commits": 110592, "reads": 248832, "reopens": 288, "processes": 72}, counts
    summaries = []
    for reopen, writers, readers in COHORTS:
        for durable, grouped in [(False, False), (True, False), (True, True)]:
            for variant in ["stack", "vector"]:
                ratios = [measured[reopen, writers, readers, durable, grouped, block, variant]["read_workload"]["latency"]["p50_ns"] /
                          measured[reopen, writers, readers, durable, grouped, block, "map"]["read_workload"]["latency"]["p50_ns"] for block in range(6)]
                summaries.append({"reopen": reopen, "writers": writers, "readers": readers, "durable": durable,
                                  "grouped": grouped, "variant": variant, "read_p50_ratios": ratios,
                                  "median_ratio": statistics.median(ratios)})
    write(output / "analysis.json", {"diagnostic_only": True, "scope": "Six descriptive blocks per cohort, no strict gate or matrix confidence claim", "rows": summaries})
    inventory_rows = list(map(json.loads, (output / "process-inventory.jsonl").read_text().splitlines()))
    contaminated = [row for row in inventory_rows if row["compilers"]]
    covered = bool(inventory_rows) and inventory_rows[0]["unix"] <= timing_started + 2 and inventory_rows[-1]["unix"] >= timing_finished - 2
    covered = covered and all(b["unix"] - a["unix"] <= 3 for a, b in zip(inventory_rows, inventory_rows[1:]))
    qualified = not contaminated and not monitor_errors and covered
    write(output / "receipt.json", {"diagnostic_only": True, "exact_checks_passed": True, "counts": dict(counts),
                                    "known_compiler_interference": contaminated, "inventory_errors": monitor_errors,
                                    "inventory_coverage_complete": covered, "timing_environment_qualified": qualified,
                                    "results_sha256": digest(output / "results.jsonl")})
    assert qualified, "Compiler overlap or missing inventory detected; timing is unqualified and samples retained."



if __name__ == "__main__":
    main()

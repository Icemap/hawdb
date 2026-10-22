#!/usr/bin/env python3
"""Retain the predeclared 40-pair, zero-increase single-stream writer gate."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--baseline-source", type=Path, required=True)
    parser.add_argument("--baseline-revision", default="a703cc0f18f549918c73f454cda521edb4f0fb13")
    parser.add_argument("--fixture-parent", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = Path(__file__).resolve().parents[1]
    binaries = {name: path.resolve(strict=True) for name, path in
                [("baseline", args.baseline), ("candidate", args.candidate)]}
    harness = source / "benches/concurrent_writers.rs"
    assert sha256(harness) == sha256(args.baseline_source / "benches/concurrent_writers.rs")
    args.fixture_parent.mkdir(parents=True, exist_ok=True)
    filesystem = subprocess.check_output(["df", "-T", str(args.fixture_parent)], text=True)
    if "tmpfs" in filesystem:
        parser.error("durable qualification requires a disk-backed fixture parent")
    args.output.mkdir(parents=True, exist_ok=False)
    source_names = sorted(set(name for name in subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"], cwd=source, text=True
    ).split("\0") if name and Path(name).suffix in {".rs", ".toml", ".lock", ".bzl", ".bazel", ".py"}))
    source_manifest = {name: sha256(source / name) for name in source_names}
    (args.output / "candidate-source.json").write_text(json.dumps(source_manifest, indent=2) + "\n")
    protocol = {
        "schema": "concurrent-writer-strict-gate-v1",
        "created_utc": datetime.now(timezone.utc).isoformat(),
        "baseline_revision": args.baseline_revision,
        "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip(),
        "candidate_status": subprocess.check_output(["git", "status", "--porcelain"], cwd=source, text=True),
        "candidate_source_manifest_sha256": sha256(args.output / "candidate-source.json"),
        "candidate_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"], cwd=source)).hexdigest(),
        "harness_sha256": sha256(harness),
        "binary_sha256": {name: sha256(path) for name, path in binaries.items()},
        "binary_paths": {name: str(path) for name, path in binaries.items()},
        "cargo_lock_sha256": {"baseline": sha256(args.baseline_source / "Cargo.lock"), "candidate": sha256(source / "Cargo.lock")},
        "effective_cargo_config_sha256": sha256(source / ".cargo/config.toml"),
        "build_profile": "bench; opt-level=3, codegen-units=1, thin LTO, strip=none; both builds invoked from candidate repository",
        "toolchain": "Rust 1.97.1", "default_features": True,
        "baseline_overlay": "identical writer harness and Cargo benchmark registration only",
        "measured_pairs": 40, "warm_up_pairs": 1, "interval_ranks_one_based": [12, 29],
        "primary_endpoints": "commit p50/p95 for non-serialized API in memory, durable ungrouped, durable grouped",
        "pass_rule": "all six unrounded upper ratio endpoints <= 1.0; no adaptive stopping or sample removal",
        "order": "baseline first for even pairs, candidate first for odd pairs; alternate --round 0/1",
        "platform": platform.platform(), "filesystem": filesystem,
        "limitations": "shared host; no exclusive machine, affinity, cache flush, or power-loss claim; median-ratio intervals assume independent pairs from continuous distributions",
    }
    (args.output / "protocol.json").write_text(json.dumps(protocol, indent=2) + "\n")
    measured = {}
    with (args.output / "results.jsonl").open("x") as raw:
        for pair in range(-1, 40):
            order = ["baseline", "candidate"] if pair < 0 or pair % 2 == 0 else ["candidate", "baseline"]
            for revision in order:
                command = [str(binaries[revision]), "--writers", "1", "--round", str(max(pair, 0) % 2),
                           "--fixture-parent", str(args.fixture_parent)]
                started = time.monotonic()
                load = os.getloadavg()
                result = subprocess.run(command, text=True, capture_output=True, check=False)
                record = {"pair": pair, "warm_up": pair < 0, "revision": revision,
                          "command": command, "elapsed_seconds": time.monotonic() - started,
                          "load_before": load, "load_after": os.getloadavg(),
                          "returncode": result.returncode, "stderr": result.stderr}
                if result.returncode:
                    record["stdout"] = result.stdout
                    (args.output / "failure.json").write_text(json.dumps(record, indent=2) + "\n")
                    raise SystemExit("benchmark failed; original failure evidence retained")
                try:
                    rows = [json.loads(line) for line in result.stdout.splitlines()]
                    assert len(rows) == 6 and all(not row["smoke"] for row in rows)
                    assert len({(row["result"]["durable"], row["result"]["grouped"], row["result"]["serialized_control"]) for row in rows}) == 6
                    for row in rows:
                        case = row["result"]
                        assert case["commits"] == case["verified_rows"] == case["commit_latency"]["count"] == 256
                        assert case["reopen_verified"] == case["durable"]
                except (ValueError, KeyError, AssertionError):
                    record["stdout"] = result.stdout
                    (args.output / "failure.json").write_text(json.dumps(record, indent=2) + "\n")
                    raise
                record["cases"] = rows
                raw.write(json.dumps(record, separators=(",", ":")) + "\n")
                raw.flush()
                if pair >= 0:
                    measured[pair, revision] = rows
                print(f"pair={pair} revision={revision} cases=6 seconds={record['elapsed_seconds']:.3f}", flush=True)
    endpoints = []
    for durable, grouped in [(False, False), (True, False), (True, True)]:
        for metric in ["p50_ns", "p95_ns"]:
            samples = {"baseline": [], "candidate": []}
            for pair in range(40):
                for revision in samples:
                    case = next(row["result"] for row in measured[pair, revision]
                                if row["result"]["durable"] == durable and row["result"]["grouped"] == grouped
                                and not row["result"]["serialized_control"])
                    samples[revision].append(case["commit_latency"][metric])
            ratios = [c / b for b, c in zip(samples["baseline"], samples["candidate"])]
            ordered = sorted(ratios)
            low, high = ordered[11], ordered[28]
            endpoints.append({"durable": durable, "grouped": grouped, "metric": metric,
                              "baseline_median_ns": statistics.median(samples["baseline"]),
                              "candidate_median_ns": statistics.median(samples["candidate"]),
                              "paired_ratios": ratios, "median_ratio": statistics.median(ratios),
                              "ratio_interval": [low, high],
                              "disposition": "pass" if high <= 1 else "regression" if low > 1 else "inconclusive"})
    receipt = {"strict_gate_passed": all(row["disposition"] == "pass" for row in endpoints),
               "measured_cases": 480, "measured_commits": 122880, "measured_reopens": 320,
               "warm_up_cases": 12, "primary_endpoints": endpoints,
               "protocol_sha256": sha256(args.output / "protocol.json"),
               "raw_sha256": sha256(args.output / "results.jsonl")}
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps({"strict_gate_passed": receipt["strict_gate_passed"]}), flush=True)


if __name__ == "__main__":
    main()

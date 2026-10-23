#!/usr/bin/env python3
"""Manual paired comparison of identical release writer harnesses; no CI lane."""

import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--pairs", type=int, default=5)
    parser.add_argument("--reopen-before-measure", action="store_true")
    args = parser.parse_args()
    assert args.pairs >= 5
    args.output.mkdir(parents=True, exist_ok=True)
    binaries = {"baseline": args.baseline.resolve(), "candidate": args.candidate.resolve()}
    protocol = {
        "pairs": args.pairs,
        "order": "baseline/candidate on even pairs, reversed on odd pairs",
        "profile": "repository bench profile, Rust 1.97.1, default features",
        "fixture": "new database per case; checkpoint before timing; one warmup process per binary",
        "durability": "SyncOnEveryWrite; group commit is explicit in grouped baseline and candidate controls",
        "reopen_before_measure": args.reopen_before_measure,
        "reader_workers": 0,
        "binaries": {name: {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()} for name, path in binaries.items()},
        "note": "Local supporting evidence, not the canonical strict writer gate or proof of remote speedup; retain every sample.",
    }
    (args.output / "protocol.json").write_text(json.dumps(protocol, indent=2) + "\n")
    samples = {}
    with gzip.open(args.output / "results.jsonl.gz", "wt") as raw:
        def run(name, pair, writers, statements, warmup=False):
            command = [str(binaries[name]), "--writers", str(writers), "--round", str(pair % 5)]
            if args.reopen_before_measure:
                command += ["--reopen-before-measure"]
            if statements is not None:
                command += ["--pessimistic-statements", str(statements)]
            started = time.monotonic_ns()
            load = os.getloadavg()
            result = subprocess.run(command, text=True, capture_output=True, timeout=180)
            receipt = {"revision": name, "pair": pair, "warmup": warmup, "command": command, "load_before": load,
                       "elapsed_ns": time.monotonic_ns() - started, "returncode": result.returncode, "stderr": result.stderr}
            records = []
            for line in result.stdout.splitlines():
                records.append(json.loads(line))
            receipt["records"] = records
            raw.write(json.dumps(receipt) + "\n")
            raw.flush()
            assert result.returncode == 0, receipt
            assert len(records) == (2 if statements is not None else 6), receipt
            for record in records:
                assert record["smoke"] is False
                case = record["result"]
                assert case["commits"] == 256
                assert case["reopen_verified"] == case["durable"]
                if warmup:
                    continue
                key = (statements, writers, case["durable"], case.get("grouped"), case.get("serialized_control"))
                samples.setdefault(key, {}).setdefault(pair, {})[name] = case

        for name in binaries:
            run(name, 0, 4, 8, warmup=True)
        for pair in range(args.pairs):
            order = list(binaries) if pair % 2 == 0 else list(reversed(binaries))
            for statements in [None, 1, 8, 32]:
                for writers in [1, 4, 8]:
                    for name in order:
                        run(name, pair, writers, statements)
            print(f"Completed pair {pair + 1}/{args.pairs}", flush=True)

    summary = []
    for (statements, writers, durable, grouped, serial), pairs in samples.items():
        row = {"statements": statements, "writers": writers, "durable": durable, "grouped": grouped, "serialized_control": serial, "pairs": len(pairs)}
        paths = [("elapsed_ns",)]
        paths += [(phase, quantile) for phase in ["commit_latency", "staging_latency" if statements else "transaction_latency"] for quantile in ["p50_ns", "p95_ns", "p99_ns"]]
        for path in paths:
            ratios = []
            baseline = []
            candidate = []
            for values in pairs.values():
                left, right = values["baseline"], values["candidate"]
                for component in path:
                    left, right = left[component], right[component]
                baseline.append(left)
                candidate.append(right)
                ratios.append(right / left)
            row["/".join(path)] = {"baseline_median": statistics.median(baseline), "candidate_median": statistics.median(candidate),
                                   "paired_ratio_median": statistics.median(ratios), "paired_ratios": ratios}
        summary.append(row)
    (args.output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")


if __name__ == "__main__":
    main()

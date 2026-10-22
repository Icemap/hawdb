#!/usr/bin/env python3
"""Run sequential same-binary Consumer controls and retain adverse samples."""

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


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fixture-parent", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--writes", type=int, default=4096)
    parser.add_argument("--quiet-samples", type=int, default=32)
    args = parser.parse_args()
    if args.rounds < 1 or args.writes < 8 or args.quiet_samples < 1:
        parser.error("positive rounds/quiet samples and at least eight writes are required")
    binary = args.binary.resolve(strict=True)
    source = Path(__file__).resolve().parents[1]
    args.output.mkdir(parents=True, exist_ok=False)
    args.fixture_parent.mkdir(parents=True, exist_ok=True)
    filesystem = subprocess.check_output(["df", "-T", str(args.fixture_parent)], text=True)
    if "tmpfs" in filesystem:
        parser.error("durable qualification requires a disk-backed fixture parent")
    source_files = sorted(set(
        name for name in subprocess.check_output(
            ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
            cwd=source, text=True).split("\0")
        if name and (Path(name).suffix in {".rs", ".toml", ".lock", ".bzl", ".bazel"}
                     or name == "scripts/measure-consumer-publication.py")
    ))
    manifest = {
        "schema": "consumer-publication-measurement-v2",
        "started_utc": datetime.now(timezone.utc).isoformat(),
        "source_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=source, text=True).strip(),
        "source_status": subprocess.check_output(["git", "status", "--porcelain"], cwd=source, text=True),
        "tracked_diff_sha256": hashlib.sha256(subprocess.check_output(["git", "diff", "HEAD"], cwd=source)).hexdigest(),
        "source_sha256": {name: digest(source / name) for name in source_files},
        "binary": str(binary), "binary_sha256": digest(binary),
        "platform": platform.platform(), "cpu_count": os.cpu_count(),
        "cpuinfo": Path("/proc/cpuinfo").read_text() if Path("/proc/cpuinfo").exists() else None,
        "filesystem": filesystem,
        "profile": "repository Cargo bench profile; no optimization overrides",
        "features": [], "default_features": True,
        "default_feature_set": ["background-maintenance", "full-text-search", "graph-analytics", "tokio-runtime", "vector-search"],
        "rounds": args.rounds, "writes_per_case": args.writes,
        "quiescent_publications_per_enabled_case": args.quiet_samples,
        "concurrent_phase_publications_per_enabled_case": 16,
        "shared_maintenance_trigger_commits_per_case": 16,
        "changefeed_retention_entries": args.writes + args.quiet_samples + 64,
        "changefeed_retention_bytes": 64 << 20,
        "shapes": [[32, 128], [512, 1024]], "writers": [1, 4, 8],
        "modes": ["disabled", "scoped-blocking", "detached"],
        "isolation": "sequential cases; shared host, no exclusive machine or cold-device claim",
    }
    (args.output / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    results = []
    with (args.output / "results.jsonl").open("x") as raw:
        for round_index in range(args.rounds):
            cases = [(docs, payload, writers, mode)
                     for docs, payload in manifest["shapes"]
                     for writers in manifest["writers"] for mode in manifest["modes"]]
            if round_index % 2:
                cases.reverse()
            for index, (docs, payload, writers, mode) in enumerate(cases):
                fixture = args.fixture_parent / f"consumer-publication-{os.getpid()}-{round_index}-{index}"
                command = [str(binary), "--mode", mode, "--writers", str(writers),
                           "--documents", str(docs), "--payload", str(payload),
                           "--writes", str(args.writes), "--quiet-samples", str(args.quiet_samples),
                           "--root", str(fixture)]
                started = time.monotonic()
                load_before = os.getloadavg()
                completed = subprocess.run(command, text=True, capture_output=True, check=False)
                if completed.returncode:
                    (args.output / "failure.json").write_text(json.dumps({
                        "command": command, "returncode": completed.returncode,
                        "stdout": completed.stdout, "stderr": completed.stderr,
                    }, indent=2) + "\n")
                    raise SystemExit(f"case failed; evidence retained in {args.output / 'failure.json'}")
                result = json.loads(completed.stdout)
                result["run"] = {"round": round_index, "command": command,
                                 "wall_seconds_including_setup": time.monotonic() - started,
                                 "load_before": load_before, "load_after": os.getloadavg()}
                results.append(result)
                raw.write(json.dumps(result, separators=(",", ":")) + "\n")
                raw.flush()
                print(f"round={round_index} docs={docs} writers={writers} mode={mode} "
                      f"p95_ms={result['foreground_write']['p95_ns'] / 1e6:.3f} "
                      f"p99_ms={result['foreground_write']['p99_ns'] / 1e6:.3f} "
                      f"active_advances={result['watermark_advances_with_active_writers']}", flush=True)
    lines = [
        "# Consumer publication measurements", "",
        "Median of per-run percentiles across repetitions; all individual samples are retained in results.jsonl.",
        "Publication callback time is a lower bound on the outer owner critical section and excludes lock-entry/drop overhead.",
        "Quiescent publication samples cannot establish simultaneous source-write/Consumer-watermark progress.", "",
        "| Documents / payload | Writers | Mode | Write p50 ms | Write p95 ms | Write p99 ms | Wait p95 ms | Quiet publish p50 ms | Writes/s | Active advances |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for docs, payload in manifest["shapes"]:
        for writers in manifest["writers"]:
            for mode in manifest["modes"]:
                group = [row for row in results if row["documents"] == docs and row["writers"] == writers and row["mode"] == mode]
                def median(metric, field):
                    return statistics.median(row[metric][field] for row in group) / 1e6
                publish = "n/a" if mode == "disabled" else f"{median('quiescent_publication_exclusive_callback', 'p50_ns'):.3f}"
                advances = ",".join(str(row["watermark_advances_with_active_writers"]) for row in group)
                throughput = statistics.median(row["writes_per_second"] for row in group)
                lines.append(f"| {docs} / {payload} B | {writers} | {mode} | "
                             f"{median('foreground_write', 'p50_ns'):.3f} | {median('foreground_write', 'p95_ns'):.3f} | "
                             f"{median('foreground_write', 'p99_ns'):.3f} | {median('writer_wait_plus_entry', 'p95_ns'):.3f} | "
                             f"{publish} | {throughput:.1f} | {advances} |")
    (args.output / "summary.md").write_text("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()

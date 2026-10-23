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


def build_evidence(artifacts, binary):
    matches = []
    for line in artifacts.read_text().splitlines():
        item = json.loads(line)
        if (item.get("reason") == "compiler-artifact"
                and item.get("executable")
                and Path(item["executable"]).resolve() == binary
                and item["target"]["name"] == "consumer_publication"):
            matches.append(item)
    if len(matches) != 1:
        raise ValueError("expected exactly one Cargo artifact for the selected Consumer executable")
    artifact = matches[0]
    profile = artifact["profile"]
    if profile["opt_level"] != "3" or profile["debug_assertions"]:
        raise ValueError("Consumer timing requires the repository's optimized bench profile")
    return {"build_artifacts_sha256": digest(artifacts),
            "features": sorted(set(artifact["features"]) - {"default"}),
            "default_features": "default" in artifact["features"],
            "cargo_profile": profile}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--build-artifacts", type=Path, required=True,
                        help="Cargo --message-format=json output for this executable")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fixture-parent", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3)
    parser.add_argument("--writes", type=int, default=4096)
    parser.add_argument("--quiet-samples", type=int, default=32)
    parser.add_argument("--sustained", action="store_true",
                        help="Run the separate detached 100,000-write checkpointed progress cohort")
    args = parser.parse_args()
    if args.rounds < 1 or args.writes < 8 or args.quiet_samples < 1:
        parser.error("positive rounds/quiet samples and at least eight writes are required")
    if args.sustained and args.writes != 100_000:
        parser.error("the sustained cohort requires --writes 100000")
    binary = args.binary.resolve(strict=True)
    build = build_evidence(args.build_artifacts.resolve(strict=True), binary)
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
        "schema": "consumer-publication-measurement-v3",
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
        **build,
        "rounds": args.rounds, "writes_per_case": args.writes,
        "quiescent_publications_per_enabled_case": args.quiet_samples,
        "sustained": args.sustained,
        "staging_advance_call_limit": 200001,
        "publication_attempt_ceiling": 4096 if args.sustained else 16,
        "checkpoint_every_foreground_writes": 16384 if args.sustained else None,
        "concurrent_phase_publications_per_enabled_case": None if args.sustained else 16,
        "shared_maintenance_trigger_commits_per_case": None if args.sustained else 16,
        "changefeed_retention_entries": args.writes + args.quiet_samples + 64 + (4096 if args.sustained else 0),
        "changefeed_retention_bytes": 64 << 20,
        "shapes": [[32, 128], [512, 1024]], "writers": [1, 4, 8],
        "modes": ["detached"] if args.sustained else ["disabled", "scoped-blocking", "detached"],
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
                if args.sustained:
                    command.extend(["--sustained", "true"])
                started = time.monotonic()
                load_before = os.getloadavg()
                completed = subprocess.run(command, text=True, capture_output=True, check=False)
                try:
                    if completed.returncode:
                        raise ValueError(f"process exited {completed.returncode}")
                    result = json.loads(completed.stdout)
                    assert (result["documents"], result["payload_bytes_per_document"],
                            result["writers"], result["writes"], result["mode"]) == (
                                docs, payload, writers, args.writes, mode)
                    attempts = result["publication_attempts"] + result["quiescent_publication_samples"]
                    for attempt in attempts:
                        assert attempt["staging_advance_call_limit"] == manifest["staging_advance_call_limit"]
                        assert 0 < attempt["staging_advance_calls"] <= manifest["staging_advance_call_limit"]
                except (ValueError, KeyError, TypeError, AssertionError) as error:
                    (args.output / "failure.json").write_text(json.dumps({
                        "command": command, "returncode": completed.returncode,
                        "stdout": completed.stdout, "stderr": completed.stderr,
                        "validation_error": str(error),
                    }, indent=2) + "\n")
                    raise SystemExit(f"case failed; evidence retained in {args.output / 'failure.json'}")
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
    receipt = {
        "source": manifest["source_head"], "cases_complete": len(results),
        "expected_cases": args.rounds * len(manifest["shapes"]) * len(manifest["writers"]) * len(manifest["modes"]),
        "process_and_exact_checks_passed": True,
        "sustained": args.sustained,
        "sustained_progress_qualified": args.sustained and all(
            row.get("sustained_workload", {}).get("qualified", False) for row in results),
        "limits": "Finite fixed work, no steady-state, starvation-freedom or latency-improvement claim. Preserve separate strict writer gates and historical adverse datasets.",
    }
    assert receipt["cases_complete"] == receipt["expected_cases"]
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    if args.sustained and not receipt["sustained_progress_qualified"]:
        raise SystemExit("sustained progress criterion not met in every declared case; complete evidence retained")


if __name__ == "__main__":
    main()

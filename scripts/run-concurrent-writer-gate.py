#!/usr/bin/env python3
"""Build the fixed baseline and candidate before the manual writer qualification."""

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tarfile

BASELINE = "a703cc0f18f549918c73f454cda521edb4f0fb13"


def build(source, manifest, target, label, output):
    environment = dict(os.environ, CARGO_TARGET_DIR=str(target))
    command = ["cargo", "bench", "--locked", "--manifest-path", str(manifest),
               "--bench", "concurrent_writers", "--no-run", "--message-format=json"]
    with (output / f"{label}-build.jsonl").open("x") as stdout, \
            (output / f"{label}-build.log").open("x") as stderr:
        subprocess.run(command, cwd=source, env=environment, stdout=stdout,
                       stderr=stderr, check=True)
    executables = []
    for line in (output / f"{label}-build.jsonl").read_text().splitlines():
        item = json.loads(line)
        if (item.get("reason") == "compiler-artifact"
                and item["target"]["name"] == "concurrent_writers"
                and item.get("executable")):
            executables.append(Path(item["executable"]))
    assert len(executables) == 1, executables
    binary = output / f"{label}-concurrent-writers"
    shutil.copy2(executables[0], binary)
    return binary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--fixture-parent", type=Path, required=True)
    parser.add_argument("--baseline-revision", default=BASELINE)
    parser.add_argument("--include-captured-wal", action="store_true",
                        help="Run an additional fixed 40-pair cohort after reopening durable fixtures")
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-f]{40}", args.baseline_revision):
        parser.error("--baseline-revision must be a full lowercase commit SHA")
    baseline_revision = args.baseline_revision
    source = Path(__file__).resolve().parents[1]
    work = args.work.resolve()
    output = args.output.resolve()
    work.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=False)
    if subprocess.run(["git", "cat-file", "-e", baseline_revision + "^{commit}"],
                      cwd=source, capture_output=True, check=False).returncode:
        subprocess.run(["git", "fetch", "--no-tags", "--depth=1", "origin", baseline_revision],
                       cwd=source, check=True)
    archive = work / "baseline.tar"
    with archive.open("xb") as contents:
        subprocess.run(["git", "archive", baseline_revision], cwd=source, stdout=contents,
                       check=True)
    baseline = work / "baseline"
    baseline.mkdir()
    with tarfile.open(archive) as contents:
        contents.extractall(baseline, filter="data")
    shutil.copy2(source / "benches/concurrent_writers.rs",
                 baseline / "benches/concurrent_writers.rs")
    manifest = baseline / "Cargo.toml"
    registration = '\n[[bench]]\nname = "concurrent_writers"\nharness = false\n'
    if 'name = "concurrent_writers"' not in manifest.read_text():
        with manifest.open("a") as file:
            file.write(registration)
    build_record = {"baseline": baseline_revision,
                    "candidate": subprocess.check_output(
                        ["git", "rev-parse", "HEAD"], cwd=source, text=True).strip(),
                    "harness_sha256": hashlib.sha256(
                        (source / "benches/concurrent_writers.rs").read_bytes()).hexdigest(),
                    "order": "build baseline, build candidate, then all timing; no concurrent builds",
                    "fixture_profiles": ["post-checkpoint-control", "reopened-captured-wal"] if args.include_captured_wal else ["post-checkpoint-control"],
                    "cohort_rule": "complete both declared cohorts even if the first strict gate fails; never pool intervals across profiles",
                    "comparison": "whole versions; identical harness, effective Cargo config and pinned toolchain; separate lockfiles and target directories"}
    (output / "build-protocol.json").write_text(json.dumps(build_record, indent=2) + "\n")
    binaries = {}
    for label, manifest in [("baseline", manifest), ("candidate", source / "Cargo.toml")]:
        print(f"Building {label}", flush=True)
        binaries[label] = build(source, manifest, work / f"{label}-target", label, output)
    cohorts = [("measurement", False)]
    if args.include_captured_wal:
        cohorts.append(("captured-measurement", True))
    receipts = []
    for directory, reopen in cohorts:
        command = ["python3", str(source / "scripts/measure-concurrent-writer-gate.py"),
                    "--baseline", str(binaries["baseline"]),
                    "--candidate", str(binaries["candidate"]),
                    "--baseline-source", str(baseline),
                    "--baseline-revision", baseline_revision,
                    "--fixture-parent", str(args.fixture_parent.resolve()),
                    "--output", str(output / directory)]
        if reopen:
            command.append("--reopen-before-measure")
        subprocess.run(command, cwd=source, check=True)
        receipts.append(json.loads((output / directory / "receipt.json").read_text()))
    if args.include_captured_wal:
        # Trace only after all untraced timing. Counts include setup and reopen;
        # retain those outputs separately and never use traced latency samples.
        tracer = shutil.which("strace")
        if tracer is None:
            raise SystemExit("captured-WAL qualification requires strace for separate syscall evidence")
        for directory, reopen in cohorts:
            for label, binary in binaries.items():
                command = [tracer, "-f", "-c", "-o", str(output / directory / f"{label}-syscalls.txt"),
                           str(binary), "--writers", "1", "--round", "0",
                           "--fixture-parent", str(args.fixture_parent.resolve())]
                if reopen:
                    command.append("--reopen-before-measure")
                with (output / directory / f"{label}-traced.jsonl").open("x") as stdout, \
                        (output / directory / f"{label}-strace.log").open("x") as stderr:
                    subprocess.run(command, cwd=source, stdout=stdout, stderr=stderr, check=True)
    if not all(receipt["strict_gate_passed"] for receipt in receipts):
        raise SystemExit("strict zero-increase gate not passed; complete evidence retained")


if __name__ == "__main__":
    main()

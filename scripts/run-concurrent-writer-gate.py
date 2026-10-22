#!/usr/bin/env python3
"""Build the fixed baseline and candidate before the manual writer qualification."""

import argparse
import hashlib
import json
import os
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
    args = parser.parse_args()
    source = Path(__file__).resolve().parents[1]
    work = args.work.resolve()
    output = args.output.resolve()
    work.mkdir(parents=True, exist_ok=False)
    output.mkdir(parents=True, exist_ok=False)
    if subprocess.run(["git", "cat-file", "-e", BASELINE + "^{commit}"],
                      cwd=source, capture_output=True, check=False).returncode:
        subprocess.run(["git", "fetch", "--no-tags", "--depth=1", "origin", BASELINE],
                       cwd=source, check=True)
    archive = work / "baseline.tar"
    with archive.open("xb") as contents:
        subprocess.run(["git", "archive", BASELINE], cwd=source, stdout=contents,
                       check=True)
    baseline = work / "baseline"
    baseline.mkdir()
    with tarfile.open(archive) as contents:
        contents.extractall(baseline, filter="data")
    shutil.copy2(source / "benches/concurrent_writers.rs",
                 baseline / "benches/concurrent_writers.rs")
    manifest = baseline / "Cargo.toml"
    registration = '\n[[bench]]\nname = "concurrent_writers"\nharness = false\n'
    assert 'name = "concurrent_writers"' not in manifest.read_text()
    with manifest.open("a") as file:
        file.write(registration)
    build_record = {"baseline": BASELINE,
                    "candidate": subprocess.check_output(
                        ["git", "rev-parse", "HEAD"], cwd=source, text=True).strip(),
                    "harness_sha256": hashlib.sha256(
                        (source / "benches/concurrent_writers.rs").read_bytes()).hexdigest(),
                    "order": "build baseline, build candidate, then all timing; no concurrent builds",
                    "comparison": "whole versions; identical harness, effective Cargo config and pinned toolchain; separate lockfiles and target directories"}
    (output / "build-protocol.json").write_text(json.dumps(build_record, indent=2) + "\n")
    binaries = {}
    for label, manifest in [("baseline", manifest), ("candidate", source / "Cargo.toml")]:
        print(f"Building {label}", flush=True)
        binaries[label] = build(source, manifest, work / f"{label}-target", label, output)
    subprocess.run(["python3", str(source / "scripts/measure-concurrent-writer-gate.py"),
                    "--baseline", str(binaries["baseline"]),
                    "--candidate", str(binaries["candidate"]),
                    "--baseline-source", str(baseline),
                    "--fixture-parent", str(args.fixture_parent.resolve()),
                    "--output", str(output / "measurement")], cwd=source, check=True)
    receipt = json.loads((output / "measurement/receipt.json").read_text())
    if not receipt["strict_gate_passed"]:
        raise SystemExit("strict zero-increase gate not passed; complete evidence retained")


if __name__ == "__main__":
    main()

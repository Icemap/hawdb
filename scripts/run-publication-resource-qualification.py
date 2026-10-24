#!/usr/bin/env python3
"""Run the fixed publication qualification on an exact, separate runtime checkout."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import threading
import time

from concurrent_writer_topology import detect

ANCHOR = "8202d2f50cc057716b47cd4080e5bb56d37ab562"
IMAGE = "rust@sha256:77fac8b98f9f46062bb680b6d25d5bcaabfc400143952ebc572e924bcbedc3fa"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def inventory():
    result = subprocess.run(
        ["ps", "-axo", "pid=,ppid=,pcpu=,comm="],
        capture_output=True,
        text=True,
        check=True,
        timeout=3,
    )
    active = []
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 3)
        if len(fields) != 4:
            continue
        pid, parent, cpu, command = fields
        name = Path(command).name
        if (
            name in ["cargo", "rustc", "clippy-driver", "bazel", "bazelisk"]
            or name.startswith("shiro")
            or (name.startswith("hawdb") and "fuzz" in name)
        ):
            active.append(
                {
                    "pid": int(pid),
                    "parent": int(parent),
                    "cpu_percent": float(cpu),
                    "executable": name,
                }
            )
    return {"unix": time.time(), "monotonic": time.monotonic(), "compilers": active}


def prepare(args):
    source = args.source.resolve()
    assert (
        subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=source, text=True
        ).strip()
        == ANCHOR
    )
    assert not subprocess.check_output(
        ["git", "status", "--porcelain"], cwd=source
    ).strip()
    args.output = args.output.resolve()
    args.work = args.work.resolve()
    assert (
        args.output != args.work
        and source not in args.output.parents
        and source not in args.work.parents
    )
    for path in [source, args.output, args.work]:
        assert "," not in str(path), "Docker bind paths must not contain commas"
    args.output.mkdir(parents=True, exist_ok=False)
    args.work.mkdir(parents=True, exist_ok=False)
    for name in ["build", "cargo", "fixtures"]:
        (args.work / name).mkdir()
    data = Path(__file__).parent / "publication-resource"
    for name in ["build.sh", "measure.sh", "summarize.py", "expected-build.json"]:
        shutil.copy2(data / name, args.output / name)
    protocol = json.loads((data / "protocol.json").read_text())
    assert protocol["source"] == ANCHOR
    assert protocol["cohorts"] == [
        {
            "name": "read-resource",
            "cases": 54,
            "writes": 4096,
            "extra": ["--with-readers", "--reads-per-reader", "512"],
        },
        {"name": "sustained", "cases": 18, "writes": 100000, "extra": ["--sustained"]},
        {"name": "controls", "cases": 54, "writes": 4096, "extra": []},
    ]
    assert protocol["rounds"] == 3 and protocol["quiet_samples"] == 32
    assert protocol["resources"] == {
        "cpus": 4,
        "memory_bytes": 6442450944,
        "image": IMAGE,
    }
    protocol.update(
        declared_utc=datetime.now(timezone.utc).isoformat(),
        architecture=platform.machine(),
        system=platform.system(),
        cpu=None if args.prepare_only else detect(),
        controller_sha256=digest(Path(__file__)),
        fixture_sha256={p.name: digest(p) for p in data.iterdir() if p.is_file()},
        host_monitor={
            "interval_seconds": 1,
            "maximum_gap_seconds": 3,
            "scope": "Known compiler/fuzz processes only; no physical exclusivity claim",
        },
        container_git={"safe_directory": "/work", "scope": "Process environment only"},
    )
    write(args.output / "protocol.json", protocol)
    files = (
        subprocess.check_output(["git", "ls-files", "-z"], cwd=source)
        .decode()
        .split("\0")
    )
    write(
        args.output / "source-freeze.json",
        {
            "head": ANCHOR,
            "files": {name: digest(source / name) for name in files if name},
        },
    )
    return source, protocol


def container(args, source, protocol, stage):
    command = [
        "docker",
        "run",
        "--name",
        "publication-" + stage + "-" + str(time.time_ns()),
        "--cpus",
        "4",
        "--memory",
        "6442450944",
        "--workdir",
        "/work",
        "--env",
        "GIT_CONFIG_COUNT=1",
        "--env",
        "GIT_CONFIG_KEY_0=safe.directory",
        "--env",
        "GIT_CONFIG_VALUE_0=/work",
        "--mount",
        "type=bind,source=" + str(source) + ",target=/work,readonly",
        "--mount",
        "type=bind,source=" + str(args.output) + ",target=/evidence",
        "--mount",
        "type=bind,source="
        + str(args.work / "build")
        + ",target=/build"
        + (",readonly" if stage == "measure" else ""),
    ]
    if stage == "build":
        command += [
            "--env",
            "CARGO_HOME=/cargo",
            "--env",
            "CARGO_TARGET_DIR=/build",
            "--mount",
            "type=bind,source=" + str(args.work / "cargo") + ",target=/cargo",
        ]
    else:
        command += [
            "--mount",
            "type=bind,source=" + str(args.work / "fixtures") + ",target=/fixtures",
        ]
    return command + [
        protocol["resources"]["image"],
        "bash",
        "/evidence/" + stage + ".sh",
    ]


def run_build(args, source, protocol):
    command = container(args, source, protocol, "build")
    write(args.output / "build-command.json", command)
    started = time.monotonic()
    with (args.output / "build-container.log").open("x") as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    assert result.returncode == 0, "Build failed; native log retained"
    roles, binary = verify_build(args.output)
    write(
        args.output / "verified-build.json",
        {
            "source": ANCHOR,
            "roles": roles,
            "binary": binary,
            "seconds": time.monotonic() - started,
            "binary_rehash": "The measurement script rehashes the read-only build binary before cases",
        },
    )


def verify_build(output):
    rows = [
        json.loads(line) for line in (output / "build.jsonl").read_text().splitlines()
    ]
    assert any(
        row.get("reason") == "build-finished" and row.get("success") for row in rows
    )
    expected = json.loads((output / "expected-build.json").read_text())
    roles = {}
    for name, kind in [
        ("hawdb_executor", "lib"),
        ("hawdb", "lib"),
        ("consumer_publication", "bench"),
    ]:
        artifacts = [
            row
            for row in rows
            if row.get("reason") == "compiler-artifact"
            and row["target"]["name"] == name
            and kind in row["target"]["kind"]
        ]
        assert len(artifacts) == 1 and not artifacts[0]["fresh"]
        row = artifacts[0]
        assert (
            row["profile"] == expected[name]["profile"]
            and row["features"] == expected[name]["features"]
        )
        assert (
            row["profile"]["opt_level"] == "3"
            and not row["profile"]["debug_assertions"]
        )
        roles[name] = {
            key: row[key]
            for key in ["profile", "features", "fresh", "executable"]
            if key in row
        }
    binary = json.loads((output / "binary.json").read_text())
    assert roles["consumer_publication"]["executable"] == binary["path"]
    return roles, binary


def inventory_qualification(rows, errors, started, finished):
    covered = (
        bool(rows)
        and rows[0]["monotonic"] <= started + 2
        and rows[-1]["monotonic"] >= finished - 2
    )
    covered = covered and all(
        0 <= b["monotonic"] - a["monotonic"] <= 3 for a, b in zip(rows, rows[1:])
    )
    observations = sum(bool(row["compilers"]) for row in rows)
    return covered, observations, covered and not errors and observations == 0


def run_measure(args, source, protocol):
    first = inventory()
    write(args.output / "host-launch.json", first)
    assert not first[
        "compilers"
    ], "Pre-effect host compiler/fuzz refusal; no timed process started"
    command = container(args, source, protocol, "measure")
    write(args.output / "measurement-command.json", command)
    stop = threading.Event()
    errors = []

    def monitor():
        try:
            with (args.output / "host-process-inventory.jsonl").open("x") as log:
                log.write(json.dumps(first) + "\n")
                log.flush()
                while not stop.wait(1):
                    log.write(json.dumps(inventory()) + "\n")
                    log.flush()
        except Exception as error:
            errors.append(repr(error))

    observer = threading.Thread(target=monitor)
    observer.start()
    started = time.monotonic()
    try:
        with (args.output / "measurement-container.log").open("x") as log:
            result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
    finally:
        finished = time.monotonic()
        stop.set()
        observer.join()
    rows = [
        json.loads(line)
        for line in (args.output / "host-process-inventory.jsonl")
        .read_text()
        .splitlines()
    ]
    covered, observations, qualified = inventory_qualification(
        rows, errors, started, finished
    )
    write(
        args.output / "host-measurement-receipt.json",
        {
            "source": ANCHOR,
            "native_returncode": result.returncode,
            "host_environment_qualified": qualified,
            "coverage_complete": covered,
            "compiler_observations": observations,
            "monitor_errors": errors,
            "samples": len(rows),
            "seconds": finished - started,
        },
    )
    with (args.output / "summary.log").open("x") as log:
        summary = subprocess.run(
            ["python3", str(args.output / "summarize.py"), str(args.output)],
            stdout=log,
            stderr=subprocess.STDOUT,
        )
    assert (
        result.returncode == 0 and summary.returncode == 0 and qualified
    ), "Qualification failed; all evidence retained without retry"
    write(
        args.output / "qualification.json",
        {
            "source": ANCHOR,
            "cases": 126,
            "publication_resource_qualified": True,
            "strict_writer_gate_accepted": False,
            "scope": "This qualification does not replace or waive the separate strict writer/read gates",
        },
    )


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--work", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prepare-only", action="store_true")
    args = parser.parse_args()
    source, protocol = prepare(args)
    if args.prepare_only:
        print(
            json.dumps({"prepared": str(args.output), "source": ANCHOR, "cases": 126})
        )
        return
    assert (
        platform.system() == "Linux"
    ), "Actual measurement requires a dedicated Linux runner"
    run_build(args, source, protocol)
    run_measure(args, source, protocol)


if __name__ == "__main__":
    main()

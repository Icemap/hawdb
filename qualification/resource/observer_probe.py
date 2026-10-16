"""Synthetic Linux memory-observer probe. Issue Number: ref #986, ref #987."""

import argparse
import gc
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tracemalloc

from observe import process_memory


BASE = "baf113543889e01abdf8ac86f3f1e89f35445f38"
OVERLAY = {
    ".github/workflows/native-bindings.yml",
    "qualification/resource/observer_probe.py",
    "qualification/resource/observe.py",
    "qualification/resource/test_observe.py",
}
PROBE_BYTES = 64 * 1024 * 1024


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def touch(buffer):
    view = memoryview(buffer).cast("B")
    try:
        for offset in range(0, view.nbytes, os.sysconf("SC_PAGE_SIZE")):
            view[offset] = 1
    finally:
        view.release()


def probe(kind):
    import pyarrow as pa

    assert sys.platform == "linux" and pa.__version__ == "26.0.0"
    # Establish a larger earlier high-water mark, then distinguish its reset
    # from both the current resident set and the smaller observed allocation.
    warmup = bytearray(2 * PROBE_BYTES)
    touch(warmup)
    del warmup
    gc.collect()
    before_reset = process_memory()
    Path("/proc/self/clear_refs").write_text("5\n")
    baseline = process_memory()
    assert before_reset["VmHWM"] - baseline["VmHWM"] >= PROBE_BYTES // 2
    assert baseline["VmHWM"] <= baseline["VmRSS"] + 1024 * 1024
    tracemalloc.start()
    traced_before, _ = tracemalloc.get_traced_memory()
    tracemalloc.reset_peak()
    pool = pa.proxy_memory_pool(pa.default_memory_pool()) if kind == "arrow" else None
    allocation = (
        pa.allocate_buffer(PROBE_BYTES, memory_pool=pool)
        if pool is not None
        else bytearray(PROBE_BYTES)
    )
    touch(allocation)
    during = process_memory()
    traced_current, traced_peak = tracemalloc.get_traced_memory()
    pool_during = (
        None
        if pool is None
        else {
            "live_bytes": pool.bytes_allocated(),
            "peak_bytes": pool.max_memory(),
            "total_bytes": pool.total_bytes_allocated(),
            "allocations": pool.num_allocations(),
        }
    )
    assert during["VmHWM"] - baseline["VmRSS"] >= 3 * PROBE_BYTES // 4
    if pool_during:
        assert pool_during["live_bytes"] >= PROBE_BYTES
        assert pool_during["peak_bytes"] >= PROBE_BYTES
    else:
        assert traced_peak - traced_before >= PROBE_BYTES
    del allocation
    gc.collect()
    pool_after = None if pool is None else pool.bytes_allocated()
    assert pool_after in (None, 0)
    after = process_memory()
    traced_after, _ = tracemalloc.get_traced_memory()
    tracemalloc.stop()
    return {
        "protocol": "hawdb-resource-observer-probe-v1",
        "kind": kind,
        "status": "ok",
        "probe_bytes": PROBE_BYTES,
        "before_reset": before_reset,
        "baseline": baseline,
        "during": during,
        "after_release": after,
        "python_traced": {
            "before": traced_before,
            "current": traced_current,
            "peak": traced_peak,
            "after_release": traced_after,
        },
        "arrow_pool_during": pool_during,
        "arrow_pool_after_release": pool_after,
        "arrow_backend": pa.default_memory_pool().backend_name,
        "python": sys.version,
        "pyarrow": pa.__version__,
        "kernel": platform.release(),
        "machine": platform.machine(),
        "database_workloads_executed": False,
        "current_head_qualification": False,
    }


def source_snapshot():
    root = Path(__file__).resolve().parents[2]

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=root, text=True).strip()

    assert not git("status", "--porcelain")
    assert set(git("diff", "--name-only", BASE, "HEAD").splitlines()) == OVERLAY
    return {
        "base": BASE,
        "source": git("rev-parse", "HEAD"),
        "tree": git("rev-parse", "HEAD^{tree}"),
        "parents": git("show", "-s", "--format=%P"),
        "overlay": sorted(OVERLAY),
        "python_executable_sha256": digest(sys.executable),
        "script_sha256": digest(__file__),
        "database_workloads_executed": False,
        "current_head_qualification": False,
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--child", choices=("python", "arrow"))
    parser.add_argument("--output", type=Path)
    parser.add_argument("--preflight-only", action="store_true")
    args = parser.parse_args()
    if args.child:
        print(json.dumps(probe(args.child)))
        return 0
    assert args.output is not None
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = source_snapshot()
    source_file = output / "source.json"
    if source_file.exists():
        assert json.loads(source_file.read_text()) == source
    source_file.write_text(json.dumps(source, indent=2) + "\n")
    if args.preflight_only:
        print(json.dumps(source))
        return 0
    import pyarrow as pa

    assert pa.__version__ == "26.0.0"
    assert not (output / "outcomes.json").exists()
    (output / "manifest.json").write_text(
        json.dumps(
            {
                "source": source,
                "pyarrow": pa.__version__,
                "native_sha256": digest(pa.lib.__file__),
                "probe_bytes": PROBE_BYTES,
                "scope": [
                    "Synthetic observer viability only; no HawDB workload or memory/performance gate qualification.",
                    "Each allocator runs in a fresh child; only that child resets its own RSS high-water mark.",
                    "RSS, Python tracing and Arrow pool counters overlap and must not be added together.",
                    "Kernel RSS is an operational measurement, not an exact allocation ledger or memory limit.",
                ],
                "references": [
                    "https://docs.kernel.org/filesystems/proc.html",
                    "https://arrow.apache.org/docs/python/generated/pyarrow.MemoryPool.html",
                ],
            },
            indent=2,
        )
        + "\n"
    )
    outcomes = []
    for kind in ("python", "arrow"):
        assert source_snapshot() == source
        command = [sys.executable, "-B", str(Path(__file__).resolve()), "--child", kind]
        with (output / (kind + ".stdout")).open("w") as stdout, (
            output / (kind + ".stderr")
        ).open("w") as stderr:
            try:
                result = subprocess.run(
                    command, stdout=stdout, stderr=stderr, timeout=120
                )
                returncode = result.returncode
                termination = "completed"
            except subprocess.TimeoutExpired:
                returncode = 124
                termination = "child_timeout"
        outcomes.append(
            {
                "kind": kind,
                "command": command,
                "exit_code": returncode,
                "termination": termination,
                "stdout_sha256": digest(output / (kind + ".stdout")),
                "stderr_sha256": digest(output / (kind + ".stderr")),
            }
        )
        (output / "outcomes.json").write_text(json.dumps(outcomes, indent=2) + "\n")
        assert source_snapshot() == source
    return int(any(outcome["exit_code"] != 0 for outcome in outcomes))


if __name__ == "__main__":
    raise SystemExit(main())

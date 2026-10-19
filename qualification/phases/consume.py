"""Phase-only qualification around the unchanged complete numeric consumer.

Native owned execution and retained prepare/pull are measured inside the library. The existing boundary
timer includes construction, pulls, descriptors and release, excluding checksum
iteration. Its residual is handoff/host/teardown overhead, not pure conversion.
"""

import argparse
import gc
import hashlib
import importlib.util
from importlib.machinery import ExtensionFileLoader
import json
from pathlib import Path
import resource
import sys
import tempfile
import time
import tracemalloc

METRICS = ("prepare_calls", "prepare_ns", "pull_calls", "pull_ns", "owned_calls", "owned_ns")


def snapshot(native):
    value = native._retained_phase_snapshot()
    assert value["protocol"] == "hawdb-retained-phase-v2"
    assert value["allocation_instrumented"] is False
    assert value["overflowed"] is False
    assert all(type(value[key]) is int and value[key] >= 0 for key in METRICS)
    return value


def attempt(native, consumer, database, mode, expected):
    before = None
    before_error = None
    try:
        before = snapshot(native)
    except Exception as error:
        # An invalid observer must not prevent the original consumer from running.
        before_error = {"stage": "before_snapshot", "type": type(error).__name__,
                        "message": str(error)}
    started = time.perf_counter_ns()
    # Keep exceptions and the original consumer's parity/ownership checks.
    result = consumer.attempt(database, mode, expected, {})
    complete_ns = time.perf_counter_ns() - started
    result.update(complete_attempt_ns=complete_ns, native_phases=None,
                  phase_control_valid=False, phase_attribution_valid=False,
                  handoff_boundary_residual_ns=None, native_work_ns=None)
    if before_error is not None:
        result["phase_validation_error"] = before_error
        return result
    try:
        after = snapshot(native)
        phases = {key: after[key] - before[key] for key in METRICS}
        result["native_phases"] = phases
        assert all(value >= 0 for value in phases.values())
        if mode == "owned":
            assert all(phases[k] == 0 for k in ("prepare_calls", "prepare_ns", "pull_calls", "pull_ns"))
        else:
            assert phases["owned_calls"] == 0 and phases["owned_ns"] == 0
        if result.get("status") == "ok":
            if mode == "owned":
                assert phases["owned_calls"] == 1 and phases["owned_ns"] > 0, phases
                native_ns = phases["owned_ns"]
                native_scope = "Rust admitted owned facade call, including native materialization."
            else:
                assert phases["prepare_calls"] == 1 and phases["pull_calls"] > 0, phases
                assert phases["prepare_ns"] > 0 and phases["pull_ns"] > 0, phases
                native_ns = phases["prepare_ns"] + phases["pull_ns"]
                native_scope = "Rust retained preparation plus demanded native pulls."
            residual = result["query_boundary_ns"] - native_ns
            assert residual >= 0, (result["query_boundary_ns"], phases)
            result.update(phase_attribution_valid=True, native_work_ns=native_ns,
                          handoff_boundary_residual_ns=residual)
            result["phase_scope"] = (native_scope +
                " Residual includes parameters, ABI/metadata, host materialization/wrappers, "
                "release and timing overhead; checksum iteration is excluded. "
                "It is not pure per-value conversion or a storage-to-host copy claim.")
        else:
            result["phase_scope"] = "Failed/refused original consumer; no boundary residual qualifies."
        result["phase_control_valid"] = True
    except Exception as error:
        # Preserve the complete consumer's original result/error and ownership
        # receipt even if this observer is invalid. Do not relabel the query.
        result["phase_validation_error"] = {"type": type(error).__name__, "message": str(error)}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--consumer", type=Path, required=True)
    parser.add_argument("--extension", type=Path, required=True)
    parser.add_argument("--consumer-sha256", required=True)
    parser.add_argument("--extension-sha256", required=True)
    parser.add_argument("--rows", type=int, required=True)
    parser.add_argument("--samples", type=int, default=31)
    parser.add_argument("--mode", choices=("owned", "retained", "arrow"), required=True)
    parser.add_argument("--backend", choices=("memory", "file"), required=True)
    parser.add_argument("--configuration", choices=("default", "large"), required=True)
    parser.add_argument("--progress-report", type=Path)
    parser.add_argument("--setup-checkpoint-rows", type=int, default=0,
                        help="Explicit supplemental fixture maintenance; zero preserves original setup")
    args = parser.parse_args()
    assert args.rows > 0 and args.samples > 0
    assert args.setup_checkpoint_rows >= 0
    assert args.setup_checkpoint_rows == 0 or args.backend == "file"
    assert not tracemalloc.is_tracing(), "refuse pre-existing allocation tracing"
    sys.path.insert(0, str(args.package.resolve()))
    loader = ExtensionFileLoader("hawdb._hawdb", str(args.extension.resolve()))
    native_spec = importlib.util.spec_from_file_location(
        "hawdb._hawdb", args.extension.resolve(), loader=loader)
    loaded_native = importlib.util.module_from_spec(native_spec)
    native_spec.loader.exec_module(loaded_native)
    sys.modules["hawdb._hawdb"] = loaded_native
    import hawdb
    import hawdb._hawdb as native
    import pyarrow
    assert pyarrow.__version__ == "26.0.0"
    assert not hasattr(native, "_boundary_profile_snapshot")
    module = Path(native.__file__).resolve()
    assert module.is_relative_to(args.package.resolve())
    assert hashlib.sha256(module.read_bytes()).hexdigest() == args.extension_sha256
    assert hashlib.sha256(args.consumer.read_bytes()).hexdigest() == args.consumer_sha256
    snapshot(native)
    spec = importlib.util.spec_from_file_location("original_consumer", args.consumer)
    consumer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(consumer)
    report = dict(rows=args.rows, backend=args.backend, mode=args.mode,
                  configuration=args.configuration, samples=args.samples,
                  pyarrow=pyarrow.__version__, records=[],
                  phase_instrumented=True, allocation_instrumented=False,
                  terminal=False,
                  setup_checkpoint_rows=args.setup_checkpoint_rows, setup_checkpoints=[],
                  consumer_sha256=args.consumer_sha256, extension_sha256=args.extension_sha256,
                  scope="Phase-only observed costs; separate from ordinary and allocation-instrumented latency. "
                        "No pure-conversion, source-reuse, process-memory-bound or speedup claim.")

    def save_progress():
        if args.progress_report is not None:
            temporary = args.progress_report.with_suffix(".tmp")
            temporary.write_text(json.dumps(report) + "\n")
            temporary.replace(args.progress_report)

    save_progress()
    with tempfile.TemporaryDirectory(prefix="hawdb-retained-phases-") as directory:
        path = None if args.backend == "memory" else Path(directory) / "db"
        database = None
        try:
            database = (native._open_large_result_qualification(path)
                        if args.configuration == "large" else hawdb.open(path))
            limits = tuple(database._qualification_limits())
            report["actual_limits"] = limits
            if args.configuration == "large":
                assert limits == (1000000, 1073741824, 1073741824, 1073741824,
                                  2147483648, 268435456, 67108864), limits
            else:
                assert limits[:4] == (0, 67108864, 268435456, 16777216)
                assert limits[4] > 0 and limits[5:] == (16777216, 67108864)
            started = time.perf_counter_ns()
            database.execute("CREATE NODE TABLE Delivery")
            database.execute("CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT")
            for start in range(0, args.rows, 512):
                database.execute("UNWIND $rows AS row CREATE (:Delivery {score: row.score})",
                                 {"rows": [{"score": n} for n in range(start, min(start + 512, args.rows))]})
                completed = min(start + 512, args.rows)
                report["setup_completed_rows"] = completed
                if (args.setup_checkpoint_rows and
                        completed // args.setup_checkpoint_rows >
                        start // args.setup_checkpoint_rows):
                    checkpoint_started = time.perf_counter_ns()
                    database.execute("CHECKPOINT")
                    report["setup_checkpoints"].append(dict(
                        completed_rows=completed, elapsed_ns=time.perf_counter_ns() - checkpoint_started))
                    save_progress()
            report["setup_ns"] = time.perf_counter_ns() - started
            expected = consumer.fixture_reference(args.rows)
            report["fixture_reference"] = expected
            report["warmup"] = attempt(native, consumer, database, args.mode, expected)
            save_progress()
            for sample in range(args.samples):
                gc.collect()
                result = attempt(native, consumer, database, args.mode, expected)
                result["sample"] = sample
                report["records"].append(result)
                save_progress()
        except Exception as error:
            report["setup_or_probe_error"] = {"type": type(error).__name__, "message": str(error)}
        finally:
            if database is not None:
                database.close()
    report["process_peak_rss_bytes"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (
        1 if sys.platform == "darwin" else 1024)
    assert hashlib.sha256(module.read_bytes()).hexdigest() == args.extension_sha256
    assert hashlib.sha256(args.consumer.read_bytes()).hexdigest() == args.consumer_sha256
    good = (len(report["records"]) == args.samples and report.get("warmup", {}).get("status") == "ok"
            and report.get("warmup", {}).get("phase_control_valid") is True
            and all(r.get("status") == "ok" and r.get("parity") is True
                    and r["phase_control_valid"] for r in report["records"]))
    good = (good and report.get("warmup", {}).get("phase_attribution_valid") is True
            and all(r["phase_attribution_valid"] for r in report["records"]))
    report["all_paths_succeeded"] = good
    if good:
        report["observed_phase_costs"] = {}
        measured = ("owned_calls", "owned_ns") if args.mode == "owned" else METRICS[:4]
        for key in ("query_boundary_ns", "elapsed_ns", "native_work_ns", "handoff_boundary_residual_ns", *measured):
            values = sorted(r["native_phases"][key] if key in METRICS else r[key]
                            for r in report["records"])
            report["observed_phase_costs"][key] = dict(
                unit="calls" if key.endswith("_calls") else "ns",
                samples=len(values), median=values[len(values) // 2],
                nearest_rank_p95=values[(95 * len(values) + 99) // 100 - 1], maximum=values[-1])
    report["terminal"] = True
    save_progress()
    print(json.dumps(report))
    return 0 if good else 1


if __name__ == "__main__":
    sys.exit(main())

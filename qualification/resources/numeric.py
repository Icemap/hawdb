"""One mode per process; original numeric consumer, independent fixture oracle."""
import argparse
import gc
import importlib.util
from importlib.machinery import ExtensionFileLoader
import json
from pathlib import Path
import resource
import sys
import tempfile
import time
import tracemalloc


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", type=Path, required=True)
    parser.add_argument("--extension", type=Path, required=True)
    parser.add_argument("--consumer", type=Path, required=True)
    parser.add_argument("--rows", type=int, required=True)
    parser.add_argument("--backend", choices=("memory", "file"), required=True)
    parser.add_argument("--configuration", choices=("default", "large"), required=True)
    parser.add_argument("--mode", choices=("owned", "retained", "arrow"), required=True)
    parser.add_argument("--samples", type=int, default=31)
    parser.add_argument("--instrumented", action="store_true")
    args = parser.parse_args()
    assert args.rows > 0 and args.samples > 0
    sys.path.insert(0, str(args.package))
    loader = ExtensionFileLoader("hawdb._hawdb", str(args.extension))
    spec = importlib.util.spec_from_file_location("hawdb._hawdb", args.extension, loader=loader)
    native = importlib.util.module_from_spec(spec); spec.loader.exec_module(native)
    sys.modules["hawdb._hawdb"] = native
    import hawdb
    import pyarrow as pa
    assert hasattr(native, "_boundary_profile_snapshot") is args.instrumented
    spec = importlib.util.spec_from_file_location("original_consumer", args.consumer)
    consumer = importlib.util.module_from_spec(spec); spec.loader.exec_module(consumer)
    report = {"rows": args.rows, "backend": args.backend, "mode": args.mode,
              "configuration": args.configuration, "instrumented": args.instrumented,
              "samples": args.samples, "pyarrow": pa.__version__, "records": [],
              "scope": "One mode per fresh process; RSS includes fixture/setup/own-mode warmup/all samples; independent oracle instead of an owned reference. Instrumented timing is separate from uninstrumented tails. No whole-memory bound."}
    with tempfile.TemporaryDirectory(prefix="hawdb-numeric-resources-") as directory:
        path = None if args.backend == "memory" else Path(directory) / "db"
        db = native._open_large_result_qualification(path) if args.configuration == "large" else hawdb.open(path)
        try:
            limits = tuple(db._qualification_limits()); report["actual_limits"] = limits
            if args.configuration == "large":
                assert limits == (1000000, 1073741824, 1073741824, 1073741824, 2147483648, 268435456, 67108864), limits
            else:
                assert limits[:4] == (0, 67108864, 268435456, 16777216) and limits[5:] == (16777216, 67108864), limits
            started = time.perf_counter_ns()
            db.execute("CREATE NODE TABLE Delivery")
            db.execute("CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT")
            for start in range(0, args.rows, 512):
                db.execute("UNWIND $rows AS row CREATE (:Delivery {score: row.score})",
                           {"rows": [{"score": value} for value in range(start, min(start + 512, args.rows))]})
            report["setup_ns"] = time.perf_counter_ns() - started
            expected = consumer.fixture_reference(args.rows); report["fixture_reference"] = expected
            report["warmup"] = consumer.attempt(db, args.mode, expected, {})
            for sample in range(args.samples):
                gc.collect()
                if args.instrumented:
                    tracemalloc.start(); tracemalloc.reset_peak()
                    before = native._boundary_profile_snapshot()
                arrow_before = pa.total_allocated_bytes()
                started = time.perf_counter_ns()
                result = consumer.attempt(db, args.mode, expected, {})
                wall = time.perf_counter_ns() - started
                arrow_after = pa.total_allocated_bytes()
                if args.instrumented:
                    after = native._boundary_profile_snapshot()
                    python_live, python_peak = tracemalloc.get_traced_memory(); tracemalloc.stop()
                    result["native_profile"] = {key: after[key] - before[key] for key in before
                        if key not in ("live_requested_bytes", "process_peak_requested_bytes")}
                    result["native_profile"].update(live_requested_bytes_before=before["live_requested_bytes"],
                        live_requested_bytes_after=after["live_requested_bytes"], process_peak_requested_bytes=after["process_peak_requested_bytes"])
                    result.update(python_traced_live_bytes=python_live, python_traced_peak_bytes=python_peak)
                result.update(sample=sample, complete_attempt_ns=wall, arrow_pool_bytes_before=arrow_before,
                              arrow_pool_bytes_after=arrow_after)
                snapshot = result.get("retained_snapshot")
                if snapshot:
                    assert all(snapshot[key] == 0 for key in ("retained_bytes", "view_handles", "buffer_owners")), snapshot
                report["records"].append(result)
        except Exception as error:
            report["setup_or_probe_error"] = {"type": type(error).__name__, "message": str(error)}
        finally:
            db.close()
    report["process_peak_rss_bytes"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * (1 if sys.platform == "darwin" else 1024)
    good = (len(report["records"]) == args.samples and report.get("warmup", {}).get("status") == "ok"
            and all(record.get("status") == "ok" and record.get("parity") is True for record in report["records"]))
    report["all_paths_succeeded"] = good
    if good:
        values = sorted(record["query_boundary_ns"] for record in report["records"])
        report["observed_latency"] = {"samples": len(values), "median_ns": values[len(values) // 2],
            "nearest_rank_p95_ns": values[(95 * len(values) + 99) // 100 - 1], "maximum_ns": values[-1],
            "scope": "Observed sample quantiles; instrumented and ordinary producers must be compared separately."}
    print(json.dumps(report))
    return 0 if good else 1


if __name__ == "__main__":
    sys.exit(main())

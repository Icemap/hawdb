"""Observe complete consumer calls with separate, non-additive counters.

Instrumentation changes execution costs. Preserved consumer timing fields must
not be mixed with ordinary performance controls.
"""

import hashlib
import json
from pathlib import Path
import platform
import sys
import tracemalloc


def process_memory():
    values = {}
    for line in Path("/proc/self/status").read_text().splitlines():
        name, _, value = line.partition(":")
        if name in ("VmRSS", "VmHWM"):
            number, unit = value.split()
            assert unit == "kB"
            values[name] = int(number) * 1024
    assert set(values) == {"VmRSS", "VmHWM"}
    return values


def pool_snapshot(pool):
    return {
        "live_bytes": pool.bytes_allocated(),
        "cumulative_peak_bytes": pool.max_memory(),
        "total_allocated_bytes": pool.total_bytes_allocated(),
        "allocations": pool.num_allocations(),
    }


def function_source(function):
    path = Path(function.__code__.co_filename).resolve()
    return {
        "name": function.__qualname__,
        "path": str(path),
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
    }


class ResourceObserver:
    """Exclusive instrumentation for a single qualification child process."""

    def __init__(self, path, native_snapshot=None):
        import pyarrow as pa

        if sys.platform != "linux":
            raise RuntimeError("resource observation requires Linux procfs")
        self.path = Path(path)
        self.path.touch(exist_ok=False)
        self.pool = pa.default_memory_pool()
        self.pyarrow_version = pa.__version__
        self.native_snapshot = native_snapshot
        self.sequence = 0
        self.active = False
        self.observer_errors = []

    def call(self, function, *args, label, **kwargs):
        if self.active or tracemalloc.is_tracing():
            raise RuntimeError("resource observation requires exclusive Python tracing")
        self.active = True
        record = {
            "protocol": "hawdb-consumer-resources-v1",
            "sequence": self.sequence,
            "label": label,
            "operation_status": "not_called",
            "instrumented": True,
            "pyarrow": self.pyarrow_version,
            "kernel": platform.release(),
            "pool_backend": self.pool.backend_name,
        }
        self.sequence += 1
        original_error = None
        observer_error = None
        started_tracing = False
        initialized = False
        try:
            record["function_source"] = function_source(function)
            record["arrow_pool_before"] = pool_snapshot(self.pool)
            record["native_before"] = (
                self.native_snapshot() if self.native_snapshot else None
            )
            tracemalloc.start()
            started_tracing = True
            record["python_traced_before"] = tracemalloc.get_traced_memory()[0]
            tracemalloc.reset_peak()
            Path("/proc/self/clear_refs").write_text("5\n")
            record["process_before"] = process_memory()
            initialized = True
            try:
                result = function(*args, **kwargs)
                record["operation_status"] = "ok"
                return result
            except BaseException as error:
                original_error = error
                record["operation_status"] = "error"
                record["operation_error"] = {
                    "type": type(error).__name__,
                    "message": str(error),
                }
                raise
        except BaseException as error:
            if original_error is None:
                observer_error = error
            raise
        finally:
            try:
                if initialized:
                    record["process_after"] = process_memory()
                    current, peak = tracemalloc.get_traced_memory()
                    record["python_traced_after"] = {
                        "current_bytes": current,
                        "peak_bytes": peak,
                    }
                    record["arrow_pool_after"] = pool_snapshot(self.pool)
                    record["native_after"] = (
                        self.native_snapshot() if self.native_snapshot else None
                    )
                    if function_source(function) != record["function_source"]:
                        raise RuntimeError("consumer source changed during observation")
            except BaseException as error:
                observer_error = error
            finally:
                if started_tracing:
                    tracemalloc.stop()
                self.active = False
            if observer_error is not None:
                record["observer_error"] = {
                    "type": type(observer_error).__name__,
                    "message": str(observer_error),
                }
            try:
                with self.path.open("a") as output:
                    output.write(json.dumps(record) + "\n")
            except BaseException as error:
                observer_error = error
            if observer_error is not None:
                self.observer_errors.append(observer_error)
                if original_error is not None:
                    original_error.add_note(
                        "Resource observer also failed: " + str(observer_error)
                    )
                else:
                    raise observer_error


def wrap_consume(module, observer):
    """Keep the original consumer body, return value and its internal timers."""
    original = module.consume

    def consume(database, mode):
        return observer.call(original, database, mode, label=mode)

    module.consume = consume
    return original

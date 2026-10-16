"""Linux integration checks for observer semantics, not database measurements."""

import argparse
import gc
import json
from pathlib import Path
import tempfile
import tracemalloc
import types
import unittest

import pyarrow as pa

from observe import ResourceObserver, wrap_consume
from observer_probe import PROBE_BYTES, touch


EVIDENCE = None


class ResourceObservationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="hawdb-observer-test-")
        self.addCleanup(self.temporary.cleanup)
        directory = EVIDENCE if EVIDENCE is not None else Path(self.temporary.name)
        self.path = directory / (self._testMethodName + ".jsonl")
        self.observer = ResourceObserver(self.path)

    def records(self):
        return [json.loads(line) for line in self.path.read_text().splitlines()]

    def test_arrow_allocation_is_visible_after_complete_release(self):
        warmup = bytearray(2 * PROBE_BYTES)
        touch(warmup)
        del warmup
        gc.collect()

        def consume():
            buffer = pa.allocate_buffer(PROBE_BYTES)
            touch(buffer)
            del buffer
            return {"rows": 3, "checksum": "unchanged"}

        result = self.observer.call(consume, label="arrow")
        self.assertEqual(result, {"rows": 3, "checksum": "unchanged"})
        row = self.records()[0]
        before, after = row["arrow_pool_before"], row["arrow_pool_after"]
        self.assertGreaterEqual(
            after["total_allocated_bytes"] - before["total_allocated_bytes"],
            PROBE_BYTES,
        )
        self.assertEqual(before["live_bytes"], after["live_bytes"])
        self.assertGreaterEqual(
            row["process_after"]["VmHWM"] - row["process_before"]["VmRSS"],
            3 * PROBE_BYTES // 4,
        )
        self.assertFalse(self.observer.observer_errors)

    def test_external_tracing_is_refused_without_invoking_consumer(self):
        calls = []

        def consume():
            calls.append(True)

        tracemalloc.start()
        try:
            with self.assertRaisesRegex(RuntimeError, "exclusive"):
                self.observer.call(consume, label="refused")
            self.assertTrue(tracemalloc.is_tracing())
            self.assertFalse(calls)
        finally:
            tracemalloc.stop()

    def test_original_exception_and_next_call_are_preserved(self):
        error = ValueError("synthetic consumer failure")

        def consume():
            raise error

        with self.assertRaises(ValueError) as caught:
            self.observer.call(consume, label="error")
        self.assertIs(caught.exception, error)
        self.assertFalse(tracemalloc.is_tracing())
        self.assertFalse(self.observer.active)
        self.assertEqual(self.observer.call(lambda: 7, label="next"), 7)
        rows = self.records()
        self.assertEqual([r["operation_status"] for r in rows], ["error", "ok"])
        self.assertEqual([r["sequence"] for r in rows], [0, 1])
        self.assertEqual(rows[0]["operation_error"]["type"], "ValueError")

    def test_returned_arrow_owner_remains_usable(self):
        def consume():
            return pa.allocate_buffer(1024 * 1024)

        buffer = self.observer.call(consume, label="retained_result")
        view = memoryview(buffer).cast("B")
        view[0] = 17
        self.assertEqual(view[0], 17)
        row = self.records()[0]
        self.assertGreaterEqual(
            row["arrow_pool_after"]["live_bytes"]
            - row["arrow_pool_before"]["live_bytes"],
            1024 * 1024,
        )
        view.release()
        del buffer
        gc.collect()

    def test_secondary_observer_error_does_not_mask_consumer_error(self):
        calls = []
        original = ValueError("original consumer error")

        def snapshot():
            calls.append(True)
            if len(calls) == 2:
                raise RuntimeError("synthetic observer error")
            return {}

        def consume():
            raise original

        self.observer.native_snapshot = snapshot
        with self.assertRaises(ValueError) as caught:
            self.observer.call(consume, label="two_errors")
        self.assertIs(caught.exception, original)
        self.assertFalse(tracemalloc.is_tracing())
        self.assertFalse(self.observer.active)
        self.assertEqual(len(self.observer.observer_errors), 1)
        row = self.records()[0]
        self.assertEqual(row["operation_error"]["type"], "ValueError")
        self.assertEqual(row["observer_error"]["type"], "RuntimeError")

    def test_wrapper_keeps_return_identity_and_consumer_timing_fields(self):
        expected = {"elapsed_ns": 123, "query_boundary_ns": 100}
        calls = []

        def consume(database, mode):
            calls.append((database, mode))
            return expected

        module = types.SimpleNamespace(consume=consume)
        self.assertIs(wrap_consume(module, self.observer), consume)
        database = object()
        self.assertIs(module.consume(database, "owned"), expected)
        self.assertEqual(calls, [(database, "owned")])
        self.assertEqual(expected, {"elapsed_ns": 123, "query_boundary_ns": 100})
        self.assertEqual(self.records()[0]["label"], "owned")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    EVIDENCE = args.output
    if EVIDENCE is not None:
        EVIDENCE.mkdir(parents=True, exist_ok=False)
    unittest.main(argv=["test_observe", "-v"])

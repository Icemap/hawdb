"""Check explicit-limit wrapper failure receipts without a database workload."""

import json
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch

import consume


class ConsumeReceiptTests(unittest.TestCase):
    def run_fixture(self, status, limits=consume.LIMITS, instrumented=False):
        temporary = tempfile.TemporaryDirectory(prefix="hawdb-consume-receipt-")
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name)
        script = directory / "consumer.py"
        script.write_text(
            "import hawdb\n"
            "def consume(database, mode):\n"
            "    return {'rows': 3, 'elapsed_ns': 123}\n"
            "def main():\n"
            "    database = hawdb.open()\n"
            "    try:\n"
            "        assert consume(database, 'owned')['elapsed_ns'] == 123\n"
            "    finally:\n"
            "        database.close()\n"
            f"    raise SystemExit({status})\n"
        )
        closed = []
        database = types.SimpleNamespace(
            _qualification_limits=lambda: limits,
            close=lambda: closed.append(True),
        )
        original_open = object()
        package = types.ModuleType("hawdb")
        package.open = original_open
        native = types.ModuleType("hawdb._hawdb")
        native._open_large_result_qualification = lambda path: database
        if instrumented:
            native._boundary_profile_snapshot = lambda: {}
        calls = []

        class Observer:
            observer_errors = []

            def __init__(self, path, snapshot):
                self.observer_errors = []
                calls.append((path, snapshot))

            def call(self, function, *args, label):
                calls.append(label)
                return function(*args)

        receipt = directory / "limits.json"
        arguments = ["--receipt", str(receipt)]
        if instrumented:
            arguments.extend(("--resources", str(directory / "resources.jsonl")))
        arguments.append(str(script))
        original_arguments = sys.argv
        with patch.dict(
            sys.modules, {"hawdb": package, "hawdb._hawdb": native}
        ), patch.object(consume, "ResourceObserver", Observer):
            try:
                consume.main(arguments)
            except BaseException as error:
                outcome = error
        self.assertIs(package.open, original_open)
        self.assertIs(sys.argv, original_arguments)
        return outcome, json.loads(receipt.read_text()), closed, calls

    def test_success_exit_and_limit_receipt_are_preserved(self):
        error, receipt, closed, calls = self.run_fixture(0)
        self.assertIsInstance(error, SystemExit)
        self.assertEqual(error.code, 0)
        self.assertEqual(receipt["limits"], [list(consume.LIMITS)])
        self.assertFalse(receipt["instrumented"])
        self.assertEqual(closed, [True])
        self.assertFalse(calls)

    def test_failed_exit_is_preserved_with_receipt(self):
        error, receipt, closed, _ = self.run_fixture(7)
        self.assertIsInstance(error, SystemExit)
        self.assertEqual(error.code, 7)
        self.assertEqual(receipt["limits"], [list(consume.LIMITS)])
        self.assertEqual(closed, [True])

    def test_limit_mismatch_closes_database_and_retains_observation(self):
        observed = (2, *consume.LIMITS[1:])
        error, receipt, closed, _ = self.run_fixture(0, limits=observed)
        self.assertIsInstance(error, RuntimeError)
        self.assertIn("limit readback differs", str(error))
        self.assertEqual(receipt["limits"], [list(observed)])
        self.assertEqual(closed, [True])

    def test_instrumented_wrapper_preserves_consumer_output(self):
        error, receipt, closed, calls = self.run_fixture(0, instrumented=True)
        self.assertIsInstance(error, SystemExit)
        self.assertEqual(error.code, 0)
        self.assertTrue(receipt["instrumented"])
        self.assertEqual(receipt["observer_errors"], [])
        self.assertEqual(calls[1:], ["owned"])
        self.assertEqual(closed, [True])


if __name__ == "__main__":
    unittest.main(verbosity=2)

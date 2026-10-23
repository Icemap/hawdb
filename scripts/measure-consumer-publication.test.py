#!/usr/bin/env python3
"""Manual preflight contracts for source-bound Consumer measurements."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    "consumer_measurement", Path(__file__).with_name("measure-consumer-publication.py"))
measurement = importlib.util.module_from_spec(spec)
spec.loader.exec_module(measurement)


class BuildEvidenceTests(unittest.TestCase):
    def test_selected_artifact_proves_nondefault_features(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = root / "consumer"
            binary.write_bytes(b"fixture")
            artifact = {"reason": "compiler-artifact", "target": {"name": "consumer_publication"},
                        "executable": str(binary), "features": ["full-text-search", "background-maintenance"],
                        "profile": {"opt_level": "3", "debug_assertions": False}}
            foreign = dict(artifact, executable=str(root / "other"), features=["default"])
            receipts = root / "build.jsonl"
            receipts.write_text("\n".join(json.dumps(row) for row in (
                {"reason": "build-script-executed"}, foreign, artifact)))
            result = measurement.build_evidence(receipts, binary)
            self.assertFalse(result["default_features"])
            self.assertEqual(result["features"], ["background-maintenance", "full-text-search"])
            self.assertEqual(result["build_artifacts_sha256"], measurement.digest(receipts))
            for rows in ([foreign], [artifact, artifact]):
                receipts.write_text("\n".join(json.dumps(row) for row in rows))
                with self.assertRaisesRegex(ValueError, "exactly one Cargo artifact"):
                    measurement.build_evidence(receipts, binary)

    def test_checked_profile_cannot_qualify_latency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = root / "consumer"
            binary.write_bytes(b"fixture")
            receipts = root / "build.jsonl"
            receipts.write_text(json.dumps({
                "reason": "compiler-artifact", "target": {"name": "consumer_publication"},
                "executable": str(binary), "features": ["default"],
                "profile": {"opt_level": "1", "debug_assertions": True}}))
            with self.assertRaisesRegex(ValueError, "optimized bench profile"):
                measurement.build_evidence(receipts, binary)


if __name__ == "__main__":
    unittest.main()

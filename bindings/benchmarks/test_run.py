# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Qualification failures must survive artifact selection and reporting."""

import contextlib
import io
import json
import os
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

import run as driver
import compare


def complete_profile(layer):
    profile = dict.fromkeys((
        "allocation_calls", "allocated_bytes", "deallocation_calls",
        "deallocated_bytes", "reallocation_calls", "live_requested_bytes_before",
        "live_requested_bytes_after", "process_peak_requested_bytes",
    ), 0)
    if layer != "rust":
        profile.update(parameter_conversion_ns=0, engine_call_ns=0, result_conversion_ns=0)
    return profile


def complete_go_host_profile(calls=1):
    return {"parameter_conversion_ns": 0, "parameter_conversion_calls": calls,
            "ffi_call_ns": 0, "ffi_calls": calls, "result_conversion_ns": 0,
            "result_conversion_calls": calls, "valid": True,
            "scope": "go_query_parameter_native_call_result_phases"}


class DriverTests(unittest.TestCase):
    def matrix(self, native_profiles, supplied_profiles, process_exit=0, read_ns=10,
               replace_artifact=False, layers="rust,python,go", record_status="ok",
               go_host_profiles=False, supplied_host_profiles=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            artifacts = {}
            for name in ("rust", "python", "go", "library", "python-extension"):
                path = root / name
                path.write_bytes(name.encode())
                artifacts[name] = path
            output = root / "result"
            arguments = ["run.py", "--output", str(output), "--sizes", "2",
                         "--cases", "select", "--backends", "memory", "--samples", "1",
                         "--layers", layers]
            for name, path in artifacts.items():
                arguments.extend(("--" + name, str(path)))
            if native_profiles:
                arguments.append("--native-profiles")
            if go_host_profiles:
                arguments.append("--go-host-profiles")
            commands = []

            def child(command, artifact):
                commands.append(command)
                job = json.loads(pathlib.Path(command[1]).read_text())
                layer = pathlib.Path(command[0]).name
                if replace_artifact:
                    artifacts["rust"].write_bytes(b"changed")
                result = {
                    "status": record_status, "process_exit": process_exit,
                    "output_rows": len(job["rows"]), "checksum": job["expected_checksum"],
                    "query_boundary_ns": 10, "write_boundary_ns": 0,
                    "read_boundary_ns": read_ns, "elapsed_ns": 20,
                    "native_profile": supplied_profiles(layer) if record_status == "ok" else None,
                }
                if supplied_host_profiles is not None and layer == "go":
                    result["go_host_profile" if record_status == "ok" else "failure_go_host_profile"] = supplied_host_profiles()
                    if record_status != "ok":
                        result["failure_go_host_profile_scope"] = "db_query_calls_including_setup_warmup_and_cleanup"
                if record_status != "ok":
                    result.update(error="warmup: query memory budget exceeded",
                                  failure_native_profile=supplied_profiles(layer),
                                  failure_profile_scope="producer_run_including_setup_warmup_and_cleanup")
                return result

            with mock.patch.object(sys, "argv", arguments), \
                    mock.patch.object(driver, "source_identity", return_value={"head": "head", "tree": "tree"}), \
                    mock.patch.object(driver, "command_text", return_value="test-runtime"), \
                    mock.patch.object(driver, "run_child", side_effect=child), \
                    contextlib.redirect_stdout(io.StringIO()):
                if replace_artifact:
                    with self.assertRaisesRegex(RuntimeError, "artifacts changed"):
                        driver.main()
                    status = 1
                else:
                    status = driver.main()
            report = json.loads((output / "report.json").read_text())
            if replace_artifact:
                self.assertEqual(len(report["records"]), 1)
                self.assertFalse(report["terminal"])
                return status, report
            for command in commands:
                if pathlib.Path(command[0]).name == "python":
                    self.assertEqual(command[2], str(artifacts["python-extension"].resolve()))
            self.assertEqual(len(report["records"]), 2 * len(layers.split(",")))
            self.assertTrue(report["terminal"])
            return status, report

    def test_go_host_profile_requires_complete_measured_calls(self):
        status, report = self.matrix(True, complete_profile, go_host_profiles=True,
                                     supplied_host_profiles=complete_go_host_profile)
        self.assertEqual(status, 0)
        self.assertTrue(report["instrumented"])
        self.assertTrue(report["go_host_profiles"])
        self.assertTrue(all(r["go_host_profile_valid"] for r in report["records"]))
        for value in (None, True, -1, 2**64):
            with self.subTest(value=value):
                def invalid():
                    return dict(complete_go_host_profile(), result_conversion_ns=value)
                status, report = self.matrix(True, complete_profile, layers="go",
                                             go_host_profiles=True, supplied_host_profiles=invalid)
                self.assertEqual(status, 1)
                self.assertEqual(report["go_host_profile_failures"], 2)
                self.assertIsNone(report["summaries"][0]["median_query_boundary_ns"])
        status, report = self.matrix(True, complete_profile, layers="go", go_host_profiles=True,
                                     supplied_host_profiles=lambda: complete_go_host_profile(0))
        self.assertEqual(status, 1)
        self.assertEqual(report["successful_records"], 0)

    def test_go_host_profile_cannot_be_reported_as_plain(self):
        status, report = self.matrix(False, lambda _: None, layers="go",
                                     supplied_host_profiles=complete_go_host_profile)
        self.assertEqual(status, 1)
        self.assertEqual(report["go_host_profile_failures"], 2)

    def test_go_host_failure_profiles_never_qualify_latency(self):
        status, report = self.matrix(True, complete_profile, layers="go", record_status="error",
                                     go_host_profiles=True, supplied_host_profiles=complete_go_host_profile)
        self.assertEqual(status, 1)
        self.assertEqual(report["successful_records"], 0)
        self.assertTrue(all(r["failure_go_host_profile_valid"] for r in report["records"]))
        self.assertIsNone(report["summaries"][0]["median_query_boundary_ns"])

    def test_language_shard_preserves_samples_and_fixture(self):
        status, report = self.matrix(False, lambda _: None, layers="go")
        self.assertEqual(status, 0)
        self.assertEqual(report["layers"], ["go"])
        self.assertEqual(report["samples"], 1)
        self.assertEqual(report["discarded_iterations"], 1)
        self.assertEqual({r["layer"] for r in report["records"]}, {"go"})
        self.assertEqual([r["iteration"] for r in report["records"]], [0, 1])
        self.assertEqual([r["discarded"] for r in report["records"]], [True, False])
        self.assertTrue(all(r["parity"] for r in report["records"]))
        self.assertEqual(len(report["summaries"]), 1)

    def test_invalid_or_duplicate_layers_refuse_before_execution(self):
        for layers in ("", "rust,rust", "unknown"):
            with self.subTest(layers=layers), \
                    mock.patch.object(sys, "argv", ["run.py", "--output", "unused", "--layers", layers]), \
                    mock.patch.object(driver, "run_child") as child, \
                    contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as exit_status:
                    driver.main()
                self.assertEqual(exit_status.exception.code, 2)
                child.assert_not_called()

    def test_fixed_ordinary_extension_is_not_instrumentation(self):
        status, report = self.matrix(False, lambda _: None)
        self.assertEqual(status, 0)
        self.assertFalse(report["instrumented"])
        self.assertTrue(report["all_paths_succeeded"])
        self.assertIsNotNone(report["python_extension_sha256"])

    def test_missing_counters_fail_even_with_matching_rows(self):
        status, report = self.matrix(True, lambda _: None)
        self.assertEqual(status, 1)
        self.assertEqual(report["parity_failures"], 0)
        self.assertEqual(report["profile_failures"], 6)
        self.assertEqual(report["successful_records"], 0)
        self.assertEqual(report["value_parity_records"], 6)
        self.assertFalse(report["all_paths_succeeded"])
        self.assertTrue(all(summary["successful_samples"] == 0 for summary in report["summaries"]))

    def test_unexpected_instrumentation_fails_ordinary_latency(self):
        status, report = self.matrix(False, complete_profile)
        self.assertEqual(status, 1)
        self.assertFalse(report["instrumented"])
        self.assertEqual(report["profile_failures"], 6)
        self.assertFalse(report["all_paths_succeeded"])

    def test_complete_instrumented_matrix_passes(self):
        status, report = self.matrix(True, complete_profile)
        self.assertEqual(status, 0)
        self.assertTrue(report["instrumented"])
        self.assertEqual(report["profile_failures"], 0)

    def test_refusal_profiles_never_supply_successful_latency_samples(self):
        status, report = self.matrix(True, complete_profile, record_status="error")
        self.assertEqual(status, 1)
        self.assertEqual(report["successful_records"], 0)
        self.assertEqual(report["refused_or_failed_records"], 6)
        self.assertEqual(report["failure_profile_failures"], 0)
        self.assertTrue(all(r["failure_profile_valid"] is True for r in report["records"]))
        self.assertTrue(all(r["profile_valid"] is None for r in report["records"]))
        self.assertTrue(all(s["median_elapsed_ns"] is None for s in report["summaries"]))

    def test_missing_refusal_counters_remain_unqualified_not_zero(self):
        status, report = self.matrix(True, lambda _: None, record_status="error")
        self.assertEqual(status, 1)
        self.assertEqual(report["failure_profile_failures"], 6)
        self.assertTrue(all(r["failure_native_profile"] is None for r in report["records"]))

    def test_refusal_profile_requires_its_own_scope_and_complete_counters(self):
        result = {"status": "error", "failure_native_profile": complete_profile("go"),
                  "failure_profile_scope": "producer_run_including_setup_warmup_and_cleanup"}
        self.assertTrue(driver.failure_profile_valid(result, "go", True))
        result["failure_profile_scope"] = "measured_operation"
        self.assertFalse(driver.failure_profile_valid(result, "go", True))
        result["failure_profile_scope"] = "producer_run_including_setup_warmup_and_cleanup"
        result["failure_native_profile"]["allocation_calls"] = False
        self.assertFalse(driver.failure_profile_valid(result, "go", True))

    def test_nonzero_exit_cannot_supply_a_successful_latency_sample(self):
        status, report = self.matrix(False, lambda _: None, process_exit=1)
        self.assertEqual(status, 1)
        self.assertEqual(report["successful_records"], 0)
        self.assertTrue(all(summary["successful_samples"] == 0 for summary in report["summaries"]))

    def test_unbalanced_phases_cannot_supply_a_successful_latency_sample(self):
        status, report = self.matrix(False, lambda _: None, read_ns=11)
        self.assertEqual(status, 1)
        self.assertEqual(report["phase_timing_failures"], 6)
        self.assertEqual(report["successful_records"], 0)

    def test_conversion_fields_and_counter_types_are_required(self):
        result = {"status": "ok", "native_profile": complete_profile("rust")}
        self.assertTrue(driver.native_profile_valid(result, "rust", True))
        self.assertFalse(driver.native_profile_valid(result, "python", True))
        result["native_profile"]["allocation_calls"] = False
        self.assertFalse(driver.native_profile_valid(result, "rust", True))

    def test_mid_child_artifact_replacement_preserves_raw_outcome(self):
        status, report = self.matrix(False, lambda _: None, replace_artifact=True)
        self.assertEqual(status, 1)
        self.assertIn("artifacts changed", report["qualification_error"])
        self.assertEqual(report["records"][0]["process_exit"], 0)
        self.assertTrue(report["records"][0]["parity"])

    def test_same_length_artifact_replacement_aborts_qualification(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = pathlib.Path(temporary) / "engine"
            path.write_bytes(b"before")
            identity = driver.artifact_identity((path,))
            path.write_bytes(b"after!")
            with self.assertRaisesRegex(RuntimeError, "artifacts changed"):
                driver.require_artifact_identity((path,), identity)


class ComparisonTests(unittest.TestCase):
    keys = {("python", "point", 1000, "file")}

    def report(self, elapsed=20):
        return {"samples": 2, "terminal": True, "discarded_iterations": 1,
                "layers": ["python"], "cases": ["point"], "sizes": [1000], "backends": ["file"],
                "records": [
            {"layer": "python", "case": "point", "size": 1000, "backend": "file",
             "iteration": iteration, "discarded": iteration == 0, "status": "ok",
             "process_exit": 0, "parity": True, "profile_valid": True,
             "phase_timing_valid": True, "elapsed_ns": elapsed,
             "query_boundary_ns": elapsed, "write_boundary_ns": 0, "read_boundary_ns": elapsed}
            for iteration in (0, 1, 2)
        ]}

    def test_complete_groups_compare_independent_read_and_write_phases(self):
        group = compare.summarize(self.report(), self.report(10), self.keys)[0]
        self.assertTrue(group["qualified"])
        self.assertEqual(group["read_boundary_ns_median_speedup"], 2)
        self.assertIsNone(group["write_boundary_ns_median_speedup"])

    def test_one_refusal_prevents_median_speedup_claim(self):
        after = self.report(10)
        after["records"][1].update(status="error", parity=None, process_exit=1)
        group = compare.summarize(self.report(), after, self.keys)[0]
        self.assertFalse(group["qualified"])
        self.assertNotIn("elapsed_ns_median_speedup", group)

    def test_missing_sample_or_nonterminal_report_cannot_qualify(self):
        for kind in ("missing", "nonterminal"):
            with self.subTest(kind=kind):
                after = self.report()
                if kind == "missing":
                    after["records"].pop()
                else:
                    after["terminal"] = False
                self.assertFalse(compare.summarize(self.report(), after, self.keys)[0]["qualified"])

    def test_failed_parity_cannot_qualify_even_with_zero_exit(self):
        after = self.report()
        after["records"][0]["parity"] = False
        self.assertFalse(compare.summarize(self.report(), after, self.keys)[0]["qualified"])

    def test_group_missing_from_both_reports_still_blocks_full_matrix(self):
        before, after = self.report(), self.report(10)
        for report in (before, after):
            report["cases"].append("select")
        requested = self.keys | {("python", "select", 1000, "file")}
        groups = compare.summarize(before, after, requested)
        self.assertEqual(len(groups), 2)
        missing = next(group for group in groups if group["case"] == "select")
        self.assertFalse(missing["qualified"])
        self.assertEqual((missing["before_samples"], missing["after_samples"]), (0, 0))
        self.assertNotIn("elapsed_ns_median_speedup", missing)
        self.assertFalse(all(group["qualified"] for group in groups))

    def test_duplicate_or_shifted_iterations_cannot_replace_a_sample(self):
        for iterations in ((1, 1), (2, 3), (True, 2)):
            with self.subTest(iterations=iterations):
                after = self.report(10)
                for record, iteration in zip(after["records"][1:], iterations):
                    record["iteration"] = iteration
                group = compare.summarize(self.report(), after, self.keys)[0]
                self.assertFalse(group["qualified"])
                self.assertNotIn("elapsed_ns_median_speedup", group)

    def test_missing_failed_or_duplicate_warmup_cannot_qualify(self):
        for kind in ("missing", "failed", "duplicate"):
            with self.subTest(kind=kind):
                after = self.report(10)
                if kind == "missing":
                    after["records"].pop(0)
                elif kind == "failed":
                    after["records"][0]["parity"] = False
                else:
                    after["records"].append(after["records"][0].copy())
                self.assertFalse(compare.summarize(self.report(), after, self.keys)[0]["qualified"])

    def test_selection_mismatch_or_unrequested_record_cannot_qualify(self):
        for kind in ("selection", "record"):
            with self.subTest(kind=kind):
                after = self.report(10)
                if kind == "selection":
                    after["sizes"] = [100000]
                else:
                    after["records"].append(dict(after["records"][1], case="select"))
                self.assertFalse(compare.summarize(self.report(), after, self.keys)[0]["qualified"])

    def test_baseline_adapter_refuses_unqualified_or_mutable_revision(self):
        compare.validate_revision(compare.BASELINE)
        for revision in ("main", "69d526a2", "f" * 40):
            with self.subTest(revision=revision):
                with self.assertRaises(ValueError):
                    compare.validate_revision(revision)


class ProducerFreezeTests(unittest.TestCase):
    def producer(self, root):
        repository = root / "repository"
        launcher = repository / "bazel-bin/bindings/benchmarks/python_boundary"
        launcher.parent.mkdir(parents=True)
        launcher.write_text(
            "#!/usr/bin/env python3\n"
            "import json, os, sys\n"
            "sys.path.insert(0, os.environ['RUNFILES_DIR'])\n"
            "import wrapper\n"
            "print(json.dumps({'status': 'ok', 'version': wrapper.VERSION}))\n")
        launcher.chmod(0o755)
        runfiles = pathlib.Path(str(launcher) + ".runfiles")
        runfiles.mkdir()
        wrapper = repository / "wrapper.py"
        wrapper.write_text("VERSION = 'baseline'\n")
        (runfiles / "wrapper.py").symlink_to(wrapper)
        (runfiles / "runtime").mkdir()
        (runfiles / "runtime/python").write_text("frozen interpreter\n")
        (runfiles / "venv").mkdir()
        (runfiles / "venv/python").symlink_to("../runtime/python")
        (runfiles / "data with space.txt").write_text("manifest path control\n")
        (runfiles / "__pycache__").mkdir()
        (runfiles / "__pycache__/wrapper.pyc").write_bytes(b"stale bytecode")
        manifest = pathlib.Path(str(launcher) + ".runfiles_manifest")
        manifest.write_text("wrapper.py " + str(wrapper) + "\n")
        (runfiles / "MANIFEST").symlink_to(manifest)
        suffix = ".dylib" if sys.platform == "darwin" else ".so"
        for relative in ("hawdb_bench_host_boundary", "boundary",
                         "bindings/ffi/libhawdb_ffi" + suffix,
                         "bindings/python/python/hawdb/_hawdb.so"):
            path = repository / "bazel-bin" / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"native producer")
        with mock.patch.object(compare.subprocess, "run"), \
                mock.patch.object(compare, "command", return_value="bazel-bin/boundary"):
            frozen = compare.build_producer(repository, root / "frozen producer")
        return frozen, launcher, wrapper

    def test_rebuilding_shared_output_cannot_replace_frozen_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            producer, launcher, wrapper = self.producer(root)
            wrapper.write_text("VERSION = 'candidate'\n")
            launcher.write_text("raise RuntimeError('replaced launcher')\n")
            compare.require_producer(producer)
            python = producer["python"]
            runfiles = pathlib.Path(python["runfiles"]["path"])
            self.assertNotEqual(python["path"], str(launcher))
            self.assertFalse((runfiles / "wrapper.py").is_symlink())
            self.assertEqual(os.readlink(runfiles / "venv/python"), "../runtime/python")
            self.assertFalse((runfiles / "__pycache__").exists())
            manifest = (runfiles / "MANIFEST").read_text()
            self.assertIn(" data\\swith\\sspace.txt " + str(runfiles / "data with space.txt"), manifest)
            self.assertIn("wrapper.py " + str(runfiles / "wrapper.py"), manifest)
            self.assertNotIn(str(wrapper), manifest)
            with mock.patch.dict(os.environ, {"RUNFILES_DIR": str(launcher) + ".runfiles"}):
                result = driver.run_child([python["path"]], root / "frozen-import")
            self.assertEqual(result["process_exit"], 0)
            self.assertEqual(result["version"], "baseline")
            compare.require_producer(producer)

    def test_changed_frozen_package_prevents_qualification(self):
        with tempfile.TemporaryDirectory() as temporary:
            producer, _, _ = self.producer(pathlib.Path(temporary))
            runfiles = pathlib.Path(producer["python"]["runfiles"]["path"])
            (runfiles / "wrapper.py").write_text("VERSION = 'changed'\n")
            with self.assertRaisesRegex(RuntimeError, "Python runfiles changed"):
                compare.require_producer(producer)


if __name__ == "__main__":
    unittest.main()

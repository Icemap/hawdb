# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

import copy
import unittest
import tempfile
import shutil
from pathlib import Path

from runner import (
    canonical_cell,
    equivalent,
    load_suite,
    selected_cases,
    ROOT,
    InfrastructureError,
)


def rows(values):
    return {
        "status": "ok",
        "columns": ["n"],
        "rows": [[v] for v in values],
        "affected": 0,
    }


class ComparisonTests(unittest.TestCase):
    def test_multiset_keeps_duplicate_multiplicity(self):
        one = canonical_cell(23, "1")
        two = canonical_cell(20, "2")
        self.assertTrue(equivalent(rows([one, one, two]), rows([two, one, one]), {}))
        self.assertFalse(equivalent(rows([one, one, two]), rows([one, two, two]), {}))

    def test_ordered_results_are_not_sorted(self):
        one = canonical_cell(20, "1")
        two = canonical_cell(20, "2")
        self.assertFalse(
            equivalent(rows([one, two]), rows([two, one]), {"ordered": True})
        )

    def test_null_boolean_text_and_integer_are_distinct(self):
        cells = [
            canonical_cell(16, None),
            canonical_cell(16, "f"),
            canonical_cell(25, ""),
            canonical_cell(20, "0"),
            canonical_cell(25, "0"),
        ]
        for i, left in enumerate(cells):
            for j, right in enumerate(cells):
                self.assertEqual(equivalent(rows([left]), rows([right]), {}), i == j)

    def test_integer_width_mapping_is_lossless(self):
        self.assertEqual(
            canonical_cell(20, "9223372036854775807")["value"], "9223372036854775807"
        )
        self.assertEqual(canonical_cell(21, "1"), canonical_cell(23, "1"))

    def test_errors_never_pass_merely_because_both_failed(self):
        pg = {"status": "error", "sqlstate": "40001"}
        hawdb = {"status": "error", "kind": "serialization_failure"}
        self.assertFalse(equivalent(pg, hawdb, {}))
        mapping = {
            "expected_error": {
                "pg_sqlstate": "40001",
                "hawdb_kind": "serialization_failure",
            }
        }
        self.assertTrue(equivalent(pg, hawdb, mapping))
        hawdb["kind"] = "execution"
        self.assertFalse(equivalent(pg, hawdb, mapping))

    def test_empty_result_width_and_affected_rows_are_compared(self):
        left = rows([])
        right = copy.deepcopy(left)
        right["columns"] = ["n", "m"]
        self.assertFalse(equivalent(left, right, {"compare_column_names": False}))
        right = copy.deepcopy(left)
        right["affected"] = 1
        self.assertFalse(equivalent(left, right, {}))

    def test_named_columns_preserve_associations(self):
        one = canonical_cell(20, "1")
        two = canonical_cell(20, "2")
        pg = {
            "status": "ok",
            "columns": ["b", "a"],
            "rows": [[two, one]],
            "affected": 0,
        }
        hawdb = {
            "status": "ok",
            "columns": ["a", "b"],
            "rows": [[one, two]],
            "affected": 0,
        }
        self.assertTrue(equivalent(pg, hawdb, {}))
        hawdb["rows"] = [[two, one]]
        self.assertFalse(equivalent(pg, hawdb, {}))

    def test_modified_upstream_file_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            shutil.copytree(ROOT / "data", root / "data")
            shutil.copy(ROOT / "POSTGRESQL-LICENSE", root / "POSTGRESQL-LICENSE")
            path = root / "data/sql/boolean.sql"
            path.write_text(path.read_text() + "\nSELECT 12345;\n")
            with self.assertRaisesRegex(InfrastructureError, "checksum mismatch"):
                load_suite(root)

    def test_provenance_and_nonempty_selection(self):
        suite = load_suite()
        cases = selected_cases(suite, ROOT / "data/schedule", "boolean/scalar")
        self.assertEqual(len(cases), 3)
        with self.assertRaises(InfrastructureError):
            selected_cases(suite, ROOT / "data/schedule", "no-such-case")


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Verify discovery coverage and Python-to-Bash argument transport on Windows."""

import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    "power_loss_shard", Path(__file__).with_name("branch-power-loss-shard.py")
)
SHARD = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SHARD)


class PowerLossShardTests(unittest.TestCase):
    def setUp(self):
        self.names = [
            SHARD.AUTOMATIC_PREFIX + name for name in SHARD.AUTOMATIC_CASES.values()
        ] + [SHARD.PREFIX + "recovery", SHARD.PREFIX + "retained_branch"]
        self.discovery = "\n".join(name + ": test" for name in self.names) + "\n"

    def test_windows_text_mode_preserves_bash_arguments_and_complete_membership(self):
        native_open = Path.open

        def windows_open(
            path, mode="r", buffering=-1, encoding=None, errors=None, newline=None
        ):
            if "w" in mode and "b" not in mode and newline is None:
                newline = "\r\n"
            return native_open(path, mode, buffering, encoding, errors, newline)

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            discovery = root / "discovery.txt"
            discovery.write_bytes(self.discovery.replace("\n", "\r\n").encode("utf-8"))
            for shard in ["full", *SHARD.AUTOMATIC_CASES, "remaining"]:
                with self.subTest(shard=shard):
                    output = root / shard
                    with patch.object(Path, "open", windows_open), patch(
                        "sys.argv",
                        ["selector", "--discovery", str(discovery),
                         "--shard", shard, "--output-dir", str(output)],
                    ):
                        SHARD.main()
                    for path in output.iterdir():
                        self.assertNotIn(b"\r", path.read_bytes(), path.name)
                    test_filter = (
                        (output / "test-filter.txt").read_bytes().rstrip(b"\n").decode()
                    )
                    # Match Bash read -r: remove LF only, preserving an erroneous CR.
                    arguments = (
                        (output / "test-arguments.txt").read_bytes().decode().split("\n")[:-1]
                    )
                    if shard in SHARD.AUTOMATIC_CASES:
                        self.assertEqual(arguments, ["--exact"])
                        selected = [name for name in self.names if name == test_filter]
                    else:
                        self.assertEqual(
                            arguments,
                            ["--skip", SHARD.AUTOMATIC_PREFIX] if shard == "remaining" else [],
                        )
                        selected = [
                            name for name in self.names if test_filter in name
                            and not (arguments and arguments[1] in name)
                        ]
                    manifest = json.loads((output / "partition.json").read_text())
                    self.assertEqual(
                        sorted(selected), sorted(manifest["partitions"][shard])
                    )
                    members = [
                        name for group, names in manifest["partitions"].items()
                        if group != "full" for name in names
                    ]
                    self.assertCountEqual(members, self.names)

    def test_invalid_discovery_fails_closed(self):
        invalid = [
            "",
            self.discovery + self.names[0] + ": test\n",
            self.discovery.replace(self.names[0] + ": test\n", ""),
            self.discovery + SHARD.AUTOMATIC_PREFIX + "unexpected: test\n",
            "\n".join(name + ": test" for name in self.names[:4]) + "\n",
        ]
        for discovery in invalid:
            with self.subTest(discovery=discovery):
                with self.assertRaises(ValueError):
                    SHARD.partition(discovery)


if __name__ == "__main__":
    unittest.main()

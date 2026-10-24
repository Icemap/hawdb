#!/usr/bin/env python3
# Copyright 2026 Nowledge
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

"""Exercise both HawkEye header forms without changing fixture bytes."""

from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SCRIPT = ROOT / "scripts/check-license-headers.py"
FULL = (ROOT / "crates/storage/src/append_table.rs").read_text().split("\n\n", 1)[0]
SHORT = "// Copyright 2026 Nowledge\n// SPDX-License-Identifier: Apache-2.0"


class LicenseHookTests(unittest.TestCase):
    def run_fixture(self, contents, expected):
        with tempfile.TemporaryDirectory(prefix=".license-test-", dir=ROOT) as directory:
            paths = []
            for name, text in contents.items():
                path = Path(directory) / name
                path.write_text(text)
                paths.append(path)
            before = {path: path.read_bytes() for path in paths}
            result = subprocess.run(
                [sys.executable, str(SCRIPT), *map(str, paths)],
                cwd=ROOT, capture_output=True, text=True,
            )
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
            self.assertEqual({path: path.read_bytes() for path in paths}, before)

    def test_both_forms_and_supported_script_styles(self):
        self.run_fixture({
            "full.rs": FULL + "\n\nfn main() {}\n",
            "short form.rs": SHORT + "\n\nfn main() {}\n",
            "script.py": "#!/usr/bin/env python3\n" + SHORT.replace("//", "#") + "\n\npass\n",
            "script.sh": "#!/usr/bin/env bash\n" + SHORT.replace("//", "#") + "\n\ntrue\n",
        }, 0)

    def test_missing_foreign_incomplete_and_mixed_headers_fail(self):
        for invalid in [
            "fn main() {}\n",
            SHORT.replace("Nowledge", "Someone Else") + "\n\nfn main() {}\n",
            SHORT.replace("Apache-2.0", "MIT") + "\n\nfn main() {}\n",
            "// Copyright 2026 Nowledge\n\nfn main() {}\n",
        ]:
            with self.subTest(header=invalid):
                self.run_fixture({"valid.rs": SHORT + "\n\n", "invalid.rs": invalid}, 1)

    def test_markdown_unsupported_and_toml_excluded(self):
        self.run_fixture({"notes.md": "# Notes\n", "config.toml": "value = 1\n"}, 0)


if __name__ == "__main__":
    unittest.main()

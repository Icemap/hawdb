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

"""Read-only HawkEye checks accepting full Apache and canonical SPDX headers."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def check(config, paths):
    result = subprocess.run(
        ["hawkeye", "check", "--config", str(config), "--output-format", "json", "--", *paths],
        capture_output=True, text=True, encoding="utf-8", cwd=ROOT,
    )
    if result.returncode not in (0, 1):
        raise RuntimeError(result.stderr or result.stdout)
    try:
        report = json.loads(result.stdout)
        return report["files"]
    except (ValueError, KeyError, TypeError) as error:
        raise RuntimeError(result.stderr or result.stdout) from error


def main(paths):
    if not paths:
        return 0
    config = ROOT / "licenserc.toml"
    report = check(config, paths)
    rejected = [item["path"] for item in report if item["outcome"] not in ("clean", "unsupported")]
    if rejected:
        # HawkEye supports one header template per config. Keep the same file
        # selection and styles for the alternative rather than parsing headers.
        text = config.read_text()
        source = 'builtin = "Apache-2.0"'
        if text.count(source) != 1 or "[files]" not in text:
            raise RuntimeError("review the SPDX alternative after changing licenserc.toml")
        text = text.replace(source, 'text = "Copyright {{ props.inception_year }} {{ props.copyright_owner }}\\nSPDX-License-Identifier: Apache-2.0"', 1)
        text = text.replace("[files]", "[files]\nroot = " + json.dumps(str(ROOT)), 1)
        with tempfile.TemporaryDirectory(prefix="hawdb-license-") as directory:
            alternative = Path(directory) / "spdx.toml"
            alternative.write_text(text)
            alternative_report = check(alternative, [str(ROOT / path) for path in rejected])
        accepted = {item["path"] for item in alternative_report if item["outcome"] == "clean"}
        rejected = [path for path in rejected if path not in accepted]
    for path in rejected:
        print(f"FAIL | license header: {path}")
    print(f"HawkEye | {len(report)} files, {len(rejected)} invalid headers")
    return int(bool(rejected))


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (OSError, RuntimeError) as error:
        print(f"HawkEye failed: {error}", file=sys.stderr)
        sys.exit(2)

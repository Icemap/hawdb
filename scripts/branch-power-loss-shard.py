#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Select a disjoint Windows power-loss partition from actual test discovery."""

import argparse
import json
from pathlib import Path
import re

from vector_recovery_cases import RECOVERY_CASES, RECOVERY_PREFIX

PREFIX = "api::tests::power_loss::"
AUTOMATIC_PREFIX = PREFIX + "automatic_checkpoint::"
ABANDONMENT_PREFIX = PREFIX + "branch_abandonment::"
AUTOMATIC_CASES = {
    "head": "automatic_head_cuts_preserve_mixed_transactions_across_three_generations",
    "manifest": "automatic_manifest_cuts_preserve_mixed_transactions_across_three_generations",
    "root": "automatic_root_cuts_preserve_mixed_transactions_across_three_generations",
    "wal-sync": "automatic_wal_sync_cuts_preserve_mixed_transactions_across_three_generations",
}


def partition(discovery: str) -> dict[str, list[str]]:
    discovery = re.sub(r"\x1b\[[0-9;]*m", "", discovery)
    names = re.findall(r"^(api::tests::power_loss::[a-zA-Z0-9_:]+): test$", discovery, re.M)
    if not names or len(names) != len(set(names)):
        raise ValueError("power-loss discovery must contain distinct, nonempty tests")
    automatic = {name for name in names if name.startswith(AUTOMATIC_PREFIX)}
    expected = {AUTOMATIC_PREFIX + name for name in AUTOMATIC_CASES.values()}
    if automatic != expected:
        raise ValueError(f"automatic checkpoint discovery changed: {automatic ^ expected}")
    recovery = {name for name in names if name.startswith(RECOVERY_PREFIX)}
    expected_recovery = set(RECOVERY_CASES.values())
    if recovery != expected_recovery:
        raise ValueError(f"vector recovery discovery changed: {recovery ^ expected_recovery}")
    abandonment = {name for name in names if name.startswith(ABANDONMENT_PREFIX)}
    if len(abandonment) != 8:
        raise ValueError("branch abandonment discovery must contain all eight cases")
    partitions = {shard: [AUTOMATIC_PREFIX + name] for shard, name in AUTOMATIC_CASES.items()}
    partitions.update({shard: [name] for shard, name in RECOVERY_CASES.items()})
    partitions["abandonment"] = sorted(abandonment)
    partitions["remaining"] = sorted(set(names) - expected - expected_recovery - abandonment)
    selected = [name for group in partitions.values() for name in group]
    if not partitions["remaining"] or len(selected) != len(set(selected)) or set(selected) != set(names):
        raise ValueError("power-loss partitions must be nonempty, disjoint and exhaustive")
    partitions["full"] = sorted(names)
    return partitions


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discovery", type=Path, required=True)
    parser.add_argument("--shard", choices=["full", *AUTOMATIC_CASES, *RECOVERY_CASES, "abandonment", "remaining"], required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    partitions = partition(args.discovery.read_text(encoding="utf-8"))
    selected = partitions[args.shard]
    if args.shard in AUTOMATIC_CASES or args.shard in RECOVERY_CASES:
        test_filter, test_arguments = selected[0], ["--exact"]
    elif args.shard == "abandonment":
        test_filter, test_arguments = ABANDONMENT_PREFIX, []
    elif args.shard == "remaining":
        test_filter, test_arguments = PREFIX, [
            "--skip", AUTOMATIC_PREFIX, "--skip", RECOVERY_PREFIX,
            "--skip", ABANDONMENT_PREFIX,
        ]
    else:
        test_filter, test_arguments = PREFIX, []
    args.output_dir.mkdir(parents=True, exist_ok=True)
    # Bash consumes these files as arguments. Windows text-mode CRLF translation
    # would leave a literal carriage return in the filter and each option.
    outputs = {
        "selected-tests.txt": "\n".join(selected) + "\n",
        "test-filter.txt": test_filter + "\n",
        "test-arguments.txt": "".join(argument + "\n" for argument in test_arguments),
        "partition.json": json.dumps(
            {"shard": args.shard, "partitions": partitions}, indent=2
        ) + "\n",
    }
    for name, contents in outputs.items():
        with (args.output_dir / name).open("w", encoding="utf-8", newline="\n") as output:
            output.write(contents)



if __name__ == "__main__":
    main()

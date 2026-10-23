#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Select a disjoint Windows power-loss partition from actual test discovery."""

import argparse
import json
from pathlib import Path
import re

PREFIX = "api::tests::power_loss::"
AUTOMATIC_PREFIX = PREFIX + "automatic_checkpoint::"
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
    partitions = {shard: [AUTOMATIC_PREFIX + name] for shard, name in AUTOMATIC_CASES.items()}
    partitions["remaining"] = sorted(set(names) - expected)
    selected = [name for group in partitions.values() for name in group]
    if not partitions["remaining"] or len(selected) != len(set(selected)) or set(selected) != set(names):
        raise ValueError("power-loss partitions must be nonempty, disjoint and exhaustive")
    partitions["full"] = sorted(names)
    return partitions


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--discovery", type=Path, required=True)
    parser.add_argument("--shard", choices=["full", *AUTOMATIC_CASES, "remaining"], required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    partitions = partition(args.discovery.read_text(encoding="utf-8"))
    selected = partitions[args.shard]
    if args.shard in AUTOMATIC_CASES:
        test_filter, test_arguments = selected[0], ["--exact"]
    elif args.shard == "remaining":
        test_filter, test_arguments = PREFIX, ["--skip", AUTOMATIC_PREFIX]
    else:
        test_filter, test_arguments = PREFIX, []
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "selected-tests.txt").write_text("\n".join(selected) + "\n", encoding="utf-8")
    (args.output_dir / "test-filter.txt").write_text(test_filter + "\n", encoding="utf-8")
    (args.output_dir / "test-arguments.txt").write_text(
        "".join(argument + "\n" for argument in test_arguments), encoding="utf-8"
    )
    (args.output_dir / "partition.json").write_text(
        json.dumps({"shard": args.shard, "partitions": partitions}, indent=2) + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()

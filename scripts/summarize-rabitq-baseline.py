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

"""Audit the retained scalar benchmark records and summarize all measured samples."""

import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path
import statistics
import struct


def require(condition, message):
    if not condition:
        raise ValueError(message)


def integer(value, minimum=0):
    return type(value) is int and value >= minimum


def sha256(path):
    result = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def audit(path, allow_partial, artifacts_root=None):
    records = [json.loads(line) for line in path.read_text().splitlines()]
    require(len(records) >= 3, "incomplete record stream")
    header = records[0]
    require(header.get("phase") == "run", "missing run header")
    require(header.get("protocol") == "hawdb.rabitq-kernel-baseline.v1", "unknown protocol")
    require(header.get("storage_io") is True, "storage-io feature was not enabled")
    require(records[-1] == {"phase": "complete"}, "missing completion marker")
    matrix = {
        f"n{n}-d{d}-b{b}": (n, d, b)
        for n, d, b in itertools.product([100_000, 1_000_000], [67, 384, 768, 1536], [1, 4])
    }
    cases = header["cases"]
    require(bool(cases) and len(cases) == len(set(cases)), "empty or repeated case list")
    require(set(cases) <= matrix.keys(), "unknown corpus")
    require(allow_partial or set(cases) == matrix.keys(), "partial matrix requires --allow-partial")
    fixtures, samples, reference_hits, queries = {}, {}, {}, {}
    for row in records[1:-1]:
        case, phase = row.get("case"), row.get("phase")
        require(case in cases, "record outside declared case list")
        n, dimension, width = matrix[case]
        if phase == "fixture":
            require(case not in fixtures, "duplicate fixture")
            require((row["documents"], row["dimension"], row["bit_width"]) == (n, dimension, width), "fixture shape mismatch")
            require((row["samples"], row["warmups"], row["top_k"]) == (5, 1, 10), "changed sampling policy")
            require(row["generator_seed"] == 0x9940_2026_8A31_725F, "changed generator seed")
            require(len(row["query_bits"]) == dimension, "query dimension mismatch")
            require(all(integer(bits) and bits <= 0xFFFFFFFF for bits in row["query_bits"]), "invalid query bits")
            require(queries.setdefault(dimension, row["query_bits"]) == row["query_bits"], "queries differ between comparable cases")
            require(integer(row["payload_bytes"], 1), "empty payload")
            fixtures[case] = row
            continue
        require(case in fixtures, "sample precedes its fixture")
        require(integer(row["sample"]) and row["sample"] <= 5, "invalid sample index")
        require(row["warmup"] is (row["sample"] == 0), "incorrect warmup marker")
        require(integer(row["elapsed_ns"], 1), "invalid elapsed time")
        require(row["kernel"] == "scalar", "baseline unexpectedly used another kernel")
        if phase == "score_codes":
            require(row["vectors"] == n, "kernel vector count mismatch")
            require(integer(row["score_bit_sum"]), "invalid score checksum")
            key = (case, phase)
        else:
            require(phase == "projection_search", "unknown record phase")
            stride = {"unfiltered": 1, "sparse_1_percent": 100, "dense_50_percent": 2}.get(row["filter"])
            require(stride is not None, "unknown allowlist")
            require(row["backend"] in ["memory", "file"], "unknown backend")
            require(row["requested_workers"] in [1, 4], "invalid requested workers")
            require(integer(row["admitted_workers"], 1) and row["admitted_workers"] <= row["requested_workers"], "invalid admitted workers")
            require(row["scored_vectors"] == n // stride, "wrong eligible vector count")
            require(integer(row["configured_working_bytes"], 1), "invalid memory budget")
            require(integer(row["admitted_working_bytes"], 1) and row["admitted_working_bytes"] <= row["configured_working_bytes"], "working bytes exceed budget")
            require(all(integer(row[name]) for name in ["payload_bytes_read", "scanned_blocks", "skipped_blocks"]), "invalid scan accounting")
            require(row["scanned_blocks"] > 0, "scan did no work")
            require(row["payload_bytes_read"] == (fixtures[case]["payload_bytes"] if row["backend"] == "file" else 0), "unexpected payload accounting")
            hits = row["hits"]
            require(len(hits) == 10, "incorrect top-k count")
            require(len({hit[0] for hit in hits}) == len(hits), "duplicate hit IDs")
            for identifier, bits in hits:
                require(integer(identifier) and identifier < n and identifier % stride == 0, "ineligible hit")
                require(integer(bits) and bits <= 0xFFFFFFFF, "invalid score bits")
                require(math.isfinite(struct.unpack("<f", struct.pack("<I", bits))[0]), "non-finite score")
            reference = reference_hits.setdefault((case, row["filter"]), hits)
            require(hits == reference, "backend/worker/sample candidate parity mismatch")
            key = (case, phase, row["backend"], row["filter"], row["requested_workers"])
        group = samples.setdefault(key, {})
        require(row["sample"] not in group, "duplicate sample")
        group[row["sample"]] = row
    require(set(fixtures) == set(cases), "missing fixture")
    expected_groups = {(case, "score_codes") for case in cases}
    expected_groups.update(
        (case, "projection_search", backend, filter_name, workers)
        for case, backend, filter_name, workers in itertools.product(
            cases, ["memory", "file"], ["unfiltered", "sparse_1_percent", "dense_50_percent"], [1, 4]
        )
    )
    require(samples.keys() == expected_groups, "missing timing groups")
    summaries = []
    for key, group in samples.items():
        require(set(group) == set(range(6)), f"incomplete warmup/repetitions: {key}")
        if key[1] == "score_codes":
            require(len({row["score_bit_sum"] for row in group.values()}) == 1, "unstable kernel checksum")
        else:
            for name in ["admitted_workers", "admitted_working_bytes", "configured_working_bytes", "payload_bytes_read", "scanned_blocks", "skipped_blocks"]:
                require(len({row[name] for row in group.values()}) == 1, f"unstable resource accounting: {key} {name}")
        elapsed = [group[index]["elapsed_ns"] for index in range(1, 6)]
        vectors = group[0].get("vectors", group[0].get("scored_vectors"))
        summaries.append({
            "group": key, "measured_elapsed_ns": elapsed,
            "warmup_elapsed_ns": group[0]["elapsed_ns"],
            "median_ns_per_vector": statistics.median(elapsed) / vectors,
            "median_vectors_per_second": vectors * 1_000_000_000 / statistics.median(elapsed),
            "min_elapsed_ns": min(elapsed), "max_elapsed_ns": max(elapsed),
            "admitted_workers": group[0].get("admitted_workers", 1),
            "admitted_working_bytes": group[0].get("admitted_working_bytes"),
        })
    for fixture in fixtures.values():
        artifact = (artifacts_root / fixture["case"] / "projection.rabitq") if artifacts_root else Path(fixture["artifact"])
        fixture["artifact_sha256"] = sha256(artifact)
        fixture["artifact_bytes"] = artifact.stat().st_size
    return {
        "state": "complete_matrix" if set(cases) == matrix.keys() else "partial_matrix",
        "source": str(path), "source_sha256": sha256(path), "record_count": len(records),
        "header": header, "fixtures": fixtures, "groups": summaries,
        "limitations": "Warm-cache wall timings; kernel checksum is not full per-vector native parity, and admitted working bytes are not RSS. No end-to-end profile share is inferred.",
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("records", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--allow-partial", action="store_true")
    parser.add_argument("--artifacts-root", type=Path, help="relocated artifact directory for re-auditing a downloaded run")
    args = parser.parse_args()
    report = audit(args.records, args.allow_partial, args.artifacts_root)
    with args.output.open("x") as output:
        json.dump(report, output, indent=2)
        output.write("\n")
    print(f"{report['state']}: {len(report['fixtures'])} corpora, {len(report['groups'])} timing groups, {report['record_count']} records")

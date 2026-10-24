from pathlib import Path
import collections
import json
import statistics

import sys

q = Path(sys.argv[1])
report = {"source": "8202d2f50cc057716b47cd4080e5bb56d37ab562", "cohorts": {}}
for name, expected, writes in [
    ("read-resource", 54, 4096),
    ("sustained", 18, 100000),
    ("controls", 54, 4096),
]:
    receipt = json.loads((q / name / "receipt.json").read_text())
    assert (
        receipt["source"] == report["source"]
        and receipt["process_and_exact_checks_passed"]
    )
    groups = collections.defaultdict(list)
    keys = set()
    advances = []
    calls = []
    rss = []
    high_water = []
    read_requests = 0
    with (q / name / "results.jsonl").open() as stream:
        for line in stream:
            row = json.loads(line)
            key = (
                row["run"]["round"],
                row["documents"],
                row["payload_bytes_per_document"],
                row["writers"],
                row["mode"],
            )
            assert key not in keys
            keys.add(key)
            assert row["writes"] == row["foreground_write"]["count"] == writes
            assert row["verified_documents"] == row["documents"]
            assert row["durability"] == "SyncOnEveryWrite"
            attempts = (
                row["publication_attempts"] + row["quiescent_publication_samples"]
            )
            calls.extend(attempt["staging_advance_calls"] for attempt in attempts)
            record = {
                "round": row["run"]["round"],
                "write": {
                    p: row["foreground_write"][p]
                    for p in ["p50_ns", "p95_ns", "p99_ns", "max_ns"]
                },
            }
            if name == "read-resource":
                assert row["read_resource_qualified"]
                reads = row["foreground_read"]
                assert (
                    reads["requests"] == row["writers"] * 512 == len(reads["samples"])
                )
                read_requests += reads["requests"]
                if row["mode"] != "disabled":
                    assert reads["completed_during_preparation_before_writer_drain"] > 0
                record["read"] = {
                    p: reads["latency"][p]
                    for p in ["p50_ns", "p95_ns", "p99_ns", "max_ns"]
                }
                record["joint_reads"] = reads[
                    "completed_during_preparation_before_writer_drain"
                ]
                observations = [row["concurrent_phase_observation"]]
                observations.extend(
                    phase
                    for attempt in attempts
                    for phase in attempt["phase_observations"]
                )
                for observation in observations:
                    for boundary in ["resources_before", "resources_after"]:
                        resource = observation[boundary]
                        assert resource["status_available"]
                        assert 0 < resource["VmRSS"] <= resource["VmHWM"]
                        assert (
                            resource["cpu_user_ticks"] >= 0
                            and resource["cpu_system_ticks"] >= 0
                        )
                        assert (
                            "io_read_bytes" in resource and "io_write_bytes" in resource
                        )
                        rss.append(resource["VmRSS"])
                        high_water.append(resource["VmHWM"])
            if name == "sustained":
                workload = row["sustained_workload"]
                assert workload["qualified"] and not workload["publisher_cap_reached"]
                assert workload["checkpoint_every_foreground_writes"] == 16384
                assert [
                    sample[0]
                    for sample in workload["checkpoint_samples_write_count_elapsed_ns"]
                ] == [16384 * i for i in range(1, 7)]
                assert row["simultaneous_committed_post_e_advances"] >= 2
                advances.append(row["simultaneous_committed_post_e_advances"])
            if row["mode"] != "disabled":
                record["quiet_publication"] = {
                    p: row["quiescent_publication_exclusive_callback"][p]
                    for p in ["p50_ns", "p95_ns", "p99_ns", "max_ns"]
                }
            groups[(row["documents"], row["writers"], row["mode"])].append(record)
    assert len(keys) == expected == receipt["cases_complete"]
    for values in groups.values():
        assert len(values) == 3
    compact = []
    for key, values in sorted(groups.items()):
        entry = {
            "documents": key[0],
            "writers": key[1],
            "mode": key[2],
            "rounds": values,
        }
        for family in ["write", "read", "quiet_publication"]:
            if family in values[0]:
                entry[family + "_median_of_run_percentiles_ns"] = {
                    p: statistics.median(v[family][p] for v in values)
                    for p in ["p50_ns", "p95_ns", "p99_ns"]
                }
        compact.append(entry)
    report["cohorts"][name] = {
        "cases": len(keys),
        "read_requests": read_requests,
        "staging_call_range": [min(calls), max(calls)],
        "advances": advances,
        "observed_boundary_rss_kib_range": [min(rss), max(rss)] if rss else None,
        "process_lifetime_hwm_kib_range": (
            [min(high_water), max(high_water)] if high_water else None
        ),
        "groups": compact,
    }
(q / "verified-summary.json").write_text(json.dumps(report, indent=2) + "\n")
for name, values in report["cohorts"].items():
    print(name, {k: v for k, v in values.items() if k != "groups"})

import json
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
rows = [json.loads(line) for line in (root / "results.jsonl").read_text().splitlines()]
assert len(rows) == 90
assert len({(r["round"], r["result"]["durable"], r["result"]["grouped"], r["result"]["serialized_control"], r["result"]["writers"]) for r in rows}) == 90
assert all(not r["smoke"] and r["result"]["commits"] == r["result"]["verified_rows"] == 256 for r in rows)
summary = []
for durable, grouped in [(False, False), (True, False), (True, True)]:
    def group(writers, control):
        return sorted([r for r in rows if r["result"]["durable"] == durable and r["result"]["grouped"] == grouped and r["result"]["serialized_control"] == control and r["result"]["writers"] == writers], key=lambda r: r["round"])
    single = group(1, False)
    for writers in [1, 4, 8]:
        concurrent, control = group(writers, False), group(writers, True)
        ratios = [c["result"]["commits_per_second"] / b["result"]["commits_per_second"] for b, c in zip(control, concurrent)]
        scaling = [c["result"]["commits_per_second"] / b["result"]["commits_per_second"] for b, c in zip(single, concurrent)]
        summary.append({"durable": durable, "grouped": grouped, "writers": writers,
            "control_tps": statistics.median(r["result"]["commits_per_second"] for r in control),
            "concurrent_tps": statistics.median(r["result"]["commits_per_second"] for r in concurrent),
            "paired_control_speedup": ratios, "median_control_speedup": statistics.median(ratios),
            "paired_single_writer_speedup": scaling, "median_single_writer_speedup": statistics.median(scaling),
            "transaction_p95_ms": statistics.median(r["result"]["transaction_latency"]["p95_ns"] for r in concurrent) / 1e6,
            "commit_p95_ms": statistics.median(r["result"]["commit_latency"]["p95_ns"] for r in concurrent) / 1e6,
            "shared_syncs": [r["result"]["group_commit"]["shared_syncs"] for r in concurrent]})
receipt = {"cases": len(rows), "commits": sum(r["result"]["commits"] for r in rows), "reopens": sum(r["result"]["reopen_verified"] for r in rows), "summary": summary}
(root / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps({k: v for k, v in receipt.items() if k != "summary"}))

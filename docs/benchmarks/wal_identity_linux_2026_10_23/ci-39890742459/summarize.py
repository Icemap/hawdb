#!/usr/bin/env python3
"""Summarize preserved cohorts without selecting or rerunning any sample."""
import gzip
import hashlib
import json
import math
from pathlib import Path
import statistics
import sys

root = Path(sys.argv[1])
for name in ['measurement', 'captured-measurement']:
    p = root / name
    protocol = json.loads((p / 'protocol.json').read_text())
    receipt = json.loads((p / 'receipt.json').read_text())
    assert hashlib.sha256((p / 'protocol.json').read_bytes()).hexdigest() == receipt['protocol_sha256']
    raw = gzip.decompress((p / 'results.jsonl.gz').read_bytes())
    assert hashlib.sha256(raw).hexdigest() == receipt['raw_sha256']
    records = [json.loads(line) for line in raw.decode().splitlines()]
    data = {(r['pair'], r['revision']): r for r in records if not r['warm_up']}
    assert len(data) == 80
    print(name, protocol['candidate_head'], 'strict_gate_passed:', receipt['strict_gate_passed'])
    for endpoint in receipt['primary_endpoints']:
        print({k: v for k, v in endpoint.items() if k != 'paired_ratios'})
    for durable, grouped in [(False, False), (True, False), (True, True)]:
        p99, throughput = {'baseline': [], 'candidate': []}, {'baseline': [], 'candidate': []}
        for pair in range(40):
            for revision in p99:
                case = next(row['result'] for row in data[pair, revision]['cases']
                            if row['result']['durable'] == durable and row['result']['grouped'] == grouped
                            and not row['result']['serialized_control'])
                values = sorted(case['commit_latency']['samples_ns'])
                assert len(values) == 256
                p99[revision].append(values[math.ceil(len(values) * .99) - 1])
                throughput[revision].append(case['commits_per_second'])
        print('secondary', durable, grouped,
              'p99_paired_median', statistics.median(c/b for b,c in zip(p99['baseline'], p99['candidate'])),
              'p99_marginal_ns', {k:statistics.median(v) for k,v in p99.items()},
              'throughput_paired_median', statistics.median(c/b for b,c in zip(throughput['baseline'], throughput['candidate'])))
    for label in ['baseline', 'candidate']:
        trace = p / f'{label}-syscalls.txt'
        print(label, [line.strip() for line in trace.read_text().splitlines()
                      if any(line.rstrip().endswith(' ' + call) for call in ['statx', 'fstat', 'fsync', 'fdatasync'])])

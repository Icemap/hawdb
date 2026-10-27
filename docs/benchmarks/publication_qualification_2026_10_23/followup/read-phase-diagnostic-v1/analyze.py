"""Summarize the predeclared local phase diagnostic without acceptance claims."""
import collections
import hashlib
import json
from pathlib import Path
import statistics

q = Path('/evidence')
raw = q / 'results.jsonl'
receipt = json.loads((q / 'receipt.json').read_text())
assert hashlib.sha256(raw.read_bytes()).hexdigest() == receipt['results_sha256']
cohorts = collections.defaultdict(dict)
for line in raw.read_text().splitlines():
    record = json.loads(line)
    assert record['returncode'] == 0
    for row in map(json.loads, record['stdout'].splitlines()):
        case = row['result']
        if case['serialized_control']:
            continue
        key = (record['cohort']['reopen'], case['writers'], case['readers'], case['durable'], case['grouped'])
        phases = {}
        for name in ['latency', 'capture_latency', 'query_latency', 'drop_latency']:
            samples = [sample for worker in case['read_workload']['workers'] for sample in worker[name]['samples_ns']]
            assert len(samples) == case['readers'] * 64
            values = sorted(samples)
            phases[name] = {f'p{percent}_ns': values[(len(values)*percent + 99)//100 - 1] for percent in [50, 95, 99]}
        assert (record['pair'], record['revision']) not in cohorts[key]
        cohorts[key][record['pair'], record['revision']] = phases
output = []
for key, records in sorted(cohorts.items()):
    assert len(records) == 8
    rows = []
    for phase in ['latency', 'capture_latency', 'query_latency', 'drop_latency']:
        samples = {revision: [records[pair, revision][phase]['p50_ns'] for pair in range(4)] for revision in ['baseline', 'candidate']}
        ratios = [candidate/base if base else None for base, candidate in zip(samples['baseline'], samples['candidate'])]
        rows.append({'phase': phase, 'process_p50_ns': samples, 'paired_ratios': ratios, 'median_paired_ratio': statistics.median(ratios) if all(value is not None for value in ratios) else None})
    output.append({'reopen': key[0], 'writers': key[1], 'readers': key[2], 'durable': key[3], 'grouped': key[4], 'phases': rows})
(q / 'analysis.json').write_text(json.dumps({'diagnostic_only': True, 'scope': 'Four pairs per predeclared cohort on local arm64; separate from uninstrumented x86_64 strict gate. No confidence or causal claim from descriptive ratios alone.', 'cohorts': output}, indent=2) + '\n')
print(json.dumps(receipt))

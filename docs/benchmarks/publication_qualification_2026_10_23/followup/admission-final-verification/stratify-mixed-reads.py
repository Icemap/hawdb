"""Post-hoc observational strata; never substitutes for the declared gate."""
import collections
import hashlib
import json
from pathlib import Path
import statistics
import sys


def quantile(values, percent):
    ordered = sorted(values)
    return ordered[(len(ordered) * percent + 99) // 100 - 1]


def strata(case):
    writes = [interval for worker in case['workers'] for interval in worker['transaction_intervals_ns']]
    start = min(x[0] for x in writes)
    finish = max(x[1] for x in writes)
    result = collections.defaultdict(list)
    for worker in case['read_workload']['workers']:
        previous = None
        assert len(worker['epochs']) == len(worker['intervals_ns']) == len(worker['latency']['samples_ns'])
        for epoch, (a, b), latency in zip(worker['epochs'], worker['intervals_ns'], worker['latency']['samples_ns']):
            assert b - a == latency and latency > 0
            phase = 'before' if b <= start else 'after' if a >= finish else 'inside' if a >= start and b <= finish else 'crossing'
            change = 'first' if previous is None else 'same_epoch' if epoch == previous else 'advanced_epoch'
            assert previous is None or epoch >= previous
            for key in ['all', 'phase/' + phase, 'epoch/' + change, 'phase/' + phase + '/' + change]:
                result[key].append(latency)
            previous = epoch
    assert len(result['all']) == case['readers'] * 64
    return result


def analyze(root):
    output = []
    for raw in sorted(Path(root).rglob('results.jsonl')):
        cohorts = collections.defaultdict(dict)
        receipt = json.loads((raw.parent / 'receipt.json').read_text())
        assert hashlib.sha256(raw.read_bytes()).hexdigest() == receipt['raw_sha256']
        for line in raw.read_text().splitlines():
            record = json.loads(line)
            if record['warm_up']:
                continue
            assert record['returncode'] == 0
            for row in record['cases']:
                case = row['result']
                if not case['readers'] or case['serialized_control']:
                    continue
                key = (case['durable'], case['grouped'])
                cohorts[key][record['pair'], record['revision']] = strata(case)
        for (durable, grouped), samples in cohorts.items():
            assert len(samples) == 80
            names = sorted({name for s in samples.values() for name in s})
            rows = []
            for name in names:
                sizes = {revision: [len(samples[pair, revision].get(name, [])) for pair in range(40)] for revision in ['baseline', 'candidate']}
                ratios = []
                for pair in range(40):
                    base = samples[pair, 'baseline'].get(name, [])
                    candidate = samples[pair, 'candidate'].get(name, [])
                    if base and candidate:
                        ratios.append(quantile(candidate, 50) / quantile(base, 50))
                rows.append({'stratum': name, 'request_counts': sizes, 'paired_nonempty': len(ratios), 'median_paired_p50_ratio': statistics.median(ratios) if ratios else None, 'paired_p50_ratios': ratios})
            output.append({'cohort': str(raw.parent.relative_to(root)), 'durable': durable, 'grouped': grouped, 'strata': rows})
    return {'interpretation': 'Post-hoc descriptive analysis only. Strata are outcomes of the workload and can differ by revision; ratios do not establish causality or replace the predeclared gate. Epoch comparison is per worker; the first read has no predecessor.', 'cohorts': output}


if __name__ == '__main__':
    print(json.dumps(analyze(Path(sys.argv[1])), indent=2))

from pathlib import Path
import collections
import hashlib
import json
import math
import statistics
import sys


def analyze(root):
    root = Path(root)
    cohorts = []
    totals = collections.Counter()
    for receipt_path in sorted(root.rglob('receipt.json')):
        folder = receipt_path.parent
        raw_path = folder / 'results.jsonl'
        if not raw_path.exists():
            continue
        receipt = json.loads(receipt_path.read_text())
        assert hashlib.sha256(raw_path.read_bytes()).hexdigest() == receipt['raw_sha256']
        assert hashlib.sha256((folder / 'protocol.json').read_bytes()).hexdigest() == receipt['protocol_sha256']
        records = [json.loads(line) for line in raw_path.read_text().splitlines()]
        measured = {}
        counts = collections.Counter()
        for record in records:
            assert record['returncode'] == 0
            assert len(record['cases']) == 6
            for row in record['cases']:
                case = row['result']
                assert case['commits'] == case['verified_rows'] == case['commit_latency']['count'] == 256
                assert case['reopen_verified'] == case['durable']
                assert sum(worker['commits'] for worker in case['workers']) == 256
                assert all(worker['commits'] > 0 for worker in case['workers'])
                if case['readers']:
                    reads = case['read_workload']
                    assert reads['requests'] == reads['latency']['count'] == case['readers'] * 64
                    assert reads['pinned_snapshot_verified']
                    assert min(reads[key] for key in ['reads_during_writer_phase', 'writes_during_reader_phase', 'intermediate_snapshot_reads']) > 0
                if not record['warm_up']:
                    counts['cases'] += 1
                    counts['commits'] += case['commits']
                    counts['reopens'] += case['reopen_verified']
                    counts['reads'] += case['read_workload']['requests'] if case['readers'] else 0
            if not record['warm_up']:
                key = (record['pair'], record['revision'])
                assert key not in measured
                measured[key] = record['cases']
        assert len(measured) == 80
        for name in ['cases', 'commits', 'reopens', 'reads']:
            assert counts[name] == receipt.get('measured_' + name, 0)
        assert receipt['cpu_constraints_unchanged']
        endpoints = []
        for endpoint in receipt['primary_endpoints']:
            samples = {'baseline': [], 'candidate': []}
            for pair in range(40):
                for revision in samples:
                    case = next(row['result'] for row in measured[pair, revision] if not row['result']['serialized_control'] and row['result']['durable'] == endpoint['durable'] and row['result']['grouped'] == endpoint['grouped'])
                    family = endpoint['family']
                    values = case['read_workload']['latency'] if family == 'read_latency' else case if family == 'workload' else case[family]
                    samples[revision].append(values[endpoint['metric']])
            ratios = [candidate / baseline for baseline, candidate in zip(samples['baseline'], samples['candidate'])]
            assert all(math.isclose(a, b, rel_tol=1e-12) for a, b in zip(ratios, endpoint['paired_ratios']))
            assert len(endpoint['paired_ratios']) == 40
            assert math.isclose(statistics.median(ratios), endpoint['median_ratio'], rel_tol=1e-12)
            endpoints.append({key: endpoint[key] for key in ['durable', 'grouped', 'family', 'metric', 'median_ratio', 'ratio_interval', 'disposition']})
        dispositions = dict(collections.Counter(row['disposition'] for row in endpoints))
        cohorts.append({'cohort': str(folder.relative_to(root)), 'counts': dict(counts), 'dispositions': dispositions, 'endpoints': endpoints})
        totals.update(counts)
    assert cohorts
    return {'artifact_root': str(root), 'totals': dict(totals), 'cohorts': cohorts}


if __name__ == '__main__':
    print(json.dumps(analyze(sys.argv[1]), indent=2))

"""Independently reconstruct the complete experimental raw data and tradeoffs."""
from pathlib import Path
import collections
import hashlib
import json
import math
import statistics
import sys


def quantile(values, percent):
    ordered = sorted(values)
    assert ordered
    return ordered[(len(ordered) * percent + 99) // 100 - 1]



def verify_overlap(case):
    read = case['read_workload']
    writes = [interval for worker in case['workers'] for interval in worker['transaction_intervals_ns']]
    reads = [interval for worker in read['workers'] for interval in worker['intervals_ns']]
    assert len(writes) == 256 and len(reads) == case['readers'] * 64
    assert {worker['worker'] for worker in case['workers']} == set(range(case['writers']))
    assert {worker['worker'] for worker in read['workers']} == set(range(case['readers']))
    assert all(len(worker['transaction_intervals_ns']) == worker['commits'] for worker in case['workers'])
    assert all(0 <= start < end for start, end in writes + reads)
    write_start, write_end = min(start for start, _ in writes), max(end for _, end in writes)
    read_start, read_end = min(start for start, _ in reads), max(end for _, end in reads)
    assert read['reads_during_writer_phase'] == sum(start >= write_start and end <= write_end for start, end in reads)
    assert read['writes_during_reader_phase'] == sum(start >= read_start and end <= read_end for start, end in writes)
    assert read['reads_during_writer_phase'] > 0 and read['writes_during_reader_phase'] > 0


def analyze(root):
    root = Path(root)
    protocol = json.loads((root / 'protocol.json').read_text())
    assert protocol['anchor'] == '823e8e9c735050dcafb3e0bf024e12f0a46ea140'
    assert protocol['variants'] == ['map', 'stack', 'vector'] and protocol['blocks'] == 6
    receipt = json.loads((root / 'receipt.json').read_text())
    raw = root / 'results.jsonl'
    assert hashlib.sha256(raw.read_bytes()).hexdigest() == receipt['results_sha256']
    manifests = {variant: json.loads((root / f'{variant}-source.json').read_text()) for variant in protocol['variants']}
    expected_changes = {'crates/executor/src/expression.rs', 'crates/executor/src/expression/projection.rs',
                        'crates/executor/src/scan.rs', 'crates/executor/src/expression/projection/tests.rs'}
    builds = {}
    for variant in protocol['variants']:
        build = json.loads((root / f'{variant}-build-receipt.json').read_text())
        assert build['profile']['opt_level'] == '3' and not build['profile']['debug_assertions']
        cargo_records = [json.loads(line) for line in (root / f'{variant}-build.jsonl').read_text().splitlines()]
        benchmark_artifacts = [row for row in cargo_records if row.get('reason') == 'compiler-artifact'
                               and row['target']['name'] == 'concurrent_writers' and row.get('executable')]
        assert len(benchmark_artifacts) == 1 and not benchmark_artifacts[0]['fresh']
        assert benchmark_artifacts[0]['profile'] == build['profile']
        assert benchmark_artifacts[0]['features'] == build['features']
        assert any(row.get('reason') == 'build-finished' and row.get('success') for row in cargo_records)
        binary = root / f'{variant}-concurrent-writers'
        if binary.exists():
            assert hashlib.sha256(binary.read_bytes()).hexdigest() == build['binary_sha256']
        if variant != 'map':
            changed = {name for name in set(manifests['map']) | set(manifests[variant])
                       if manifests['map'].get(name) != manifests[variant].get(name)}
            assert changed == expected_changes, (variant, changed)
        builds[variant] = build
    assert len({row['target'] for row in builds.values()}) == 3
    assert len({row['binary_sha256'] for row in builds.values()}) == 3
    assert all(row['profile'] == builds['map']['profile'] and row['features'] == builds['map']['features'] for row in builds.values())
    counts = collections.Counter()
    measured = {}
    process_keys = set()
    process_windows = []
    plans = collections.defaultdict(set)
    projection_rows = collections.defaultdict(set)
    for line in raw.read_text().splitlines():
        record = json.loads(line)
        assert record['returncode'] == 0
        cohort = record['reopen'], record['writers'], record['readers']
        assert list(cohort) in protocol['cohorts']
        key = cohort + (record['block'], record['variant'])
        assert key not in process_keys
        process_keys.add(key)
        process_windows.append((record['started_unix'], record['finished_unix']))
        cases = [json.loads(row) for row in record['stdout'].splitlines()]
        modes = {(row['result']['durable'], row['result']['grouped'], row['result']['serialized_control']) for row in cases}
        assert len(cases) == len(modes) == 6
        assert modes == {(False, False, serial) for serial in [False, True]} | {(True, grouped, serial) for grouped in [False, True] for serial in [False, True]}
        for row in cases:
            assert row['schema'] == 'concurrent-writers-query-stages-diagnostic-v1'
            assert not row['smoke'] and row['round'] == record['block'] % 2
            case = row['result']
            assert case['writers'] == record['writers'] and case['readers'] == record['readers']
            assert case['commits'] == case['verified_rows'] == 256
            assert len(case['workers']) == case['writers'] and sum(worker['commits'] for worker in case['workers']) == 256
            assert all(worker['commits'] > 0 for worker in case['workers'])
            assert case['reopen_verified'] == case['durable']
            assert case['reopened_before_measure'] == (case['durable'] and record['reopen'])
            preflight = case['preflight']
            assert isinstance(preflight['physical_plan'], str) and preflight['physical_plan']
            borrowed_rows = preflight['borrowed_node_projection_rows']
            assert isinstance(borrowed_rows, int) and not isinstance(borrowed_rows, bool) and borrowed_rows >= 0
            verify_overlap(case)
            read = case['read_workload']
            assert read['pinned_snapshot_verified'] and len(read['workers']) == case['readers']
            assert len({worker['worker'] for worker in read['workers']}) == case['readers']
            assert read['requests'] == case['readers'] * 64
            assert all(read[name] > 0 for name in ['reads_during_writer_phase', 'writes_during_reader_phase', 'intermediate_snapshot_reads'])
            for worker in read['workers']:
                total = worker['latency']['samples_ns']
                phases = [worker[name]['samples_ns'] for name in ['capture_latency', 'query_latency', 'drop_latency']]
                assert all(len(values) == 64 for values in phases + [total, worker['epochs'], worker['intervals_ns'], worker['query_stage_samples_ns']])
                assert all(end - start == elapsed for (start, end), elapsed in zip(worker['intervals_ns'], total))
                assert all(sum(parts) == elapsed for parts, elapsed in zip(zip(*phases), total))
                assert all(len(stages) == 3 and min(stages) > 0 and sum(stages) <= elapsed for stages, elapsed in zip(worker['query_stage_samples_ns'], phases[1]))
                assert worker['epochs'] == sorted(worker['epochs'])
            all_reads = [sample for worker in read['workers'] for sample in worker['latency']['samples_ns']]
            assert len(all_reads) == read['requests']
            for family in [case['commit_latency'], case['transaction_latency']]:
                assert len(family['samples_ns']) == family['count'] == 256
                for percent in [50, 95, 99]:
                    assert quantile(family['samples_ns'], percent) == family[f'p{percent}_ns']
            for percent in [50, 95, 99]:
                assert quantile(all_reads, percent) == read['latency'][f'p{percent}_ns']
            assert math.isclose(case['commits_per_second'], 256e9 / case['elapsed_ns'], rel_tol=1e-12)
            assert math.isclose(read['requests_per_second'], len(all_reads) * 1e9 / case['elapsed_ns'], rel_tol=1e-12)
            counts.update(cases=1, commits=256, reads=len(all_reads), reopens=int(case['durable']))
            if not case['serialized_control']:
                case_key = cohort + (case['durable'], case['grouped'], record['block'], record['variant'])
                assert case_key not in measured
                measured[case_key] = case
                plan_key = cohort + (case['durable'], case['grouped'], record['variant'])
                plans[plan_key].add(preflight['physical_plan'])
                projection_rows[plan_key].add(borrowed_rows)
        counts['processes'] += 1
    expected_processes = {tuple(cohort) + (block, variant) for cohort in protocol['cohorts'] for block in range(6) for variant in protocol['variants']}
    assert process_keys == expected_processes
    assert dict(counts) == {'cases': 432, 'commits': 110592, 'reads': 248832, 'reopens': 288, 'processes': 72}
    assert receipt['counts'] == dict(counts)
    inventory = list(map(json.loads, (root / 'process-inventory.jsonl').read_text().splitlines()))
    assert inventory and not receipt['inventory_errors']
    assert all(not row['compilers'] for row in inventory)
    assert inventory[0]['unix'] <= min(start for start, _ in process_windows) + 2
    assert inventory[-1]['unix'] >= max(end for _, end in process_windows) - 2
    assert all(b['unix'] - a['unix'] <= 3 for a, b in zip(inventory, inventory[1:]))
    assert receipt['timing_environment_qualified'] and receipt['inventory_coverage_complete']
    comparisons = []
    for cohort in map(tuple, protocol['cohorts']):
        for durable, grouped in [(False, False), (True, False), (True, True)]:
            for variant in ['stack', 'vector']:
                for family in ['read', 'commit', 'transaction', 'execute']:
                    for percent in [50, 95, 99]:
                        paired = []
                        for block in range(6):
                            values = []
                            for label in ['map', variant]:
                                case = measured[cohort + (durable, grouped, block, label)]
                                samples = (case['read_workload']['latency']['samples_ns'] if family == 'read' else
                                           [parts[2] for worker in case['read_workload']['workers'] for parts in worker['query_stage_samples_ns']] if family == 'execute' else
                                           case[family + '_latency']['samples_ns'])
                                values.append(quantile(samples, percent))
                            paired.append(values[1] / values[0])
                        comparisons.append({'cohort': cohort, 'durable': durable, 'grouped': grouped, 'variant': variant,
                                            'family': family, 'percentile': percent, 'paired_ratios': paired, 'median_ratio': statistics.median(paired)})
    return {'diagnostic_only': True, 'strict_acceptance_claim': False, 'counts': dict(counts), 'builds': builds,
            'plans': [{'cohort': key, 'plans': sorted(values),
                       'borrowed_projection_rows_observed': sorted(projection_rows[key]),
                       'borrowed_projection_path_observed_in_every_preflight': min(projection_rows[key]) > 0}
                      for key, values in plans.items()], 'comparisons': comparisons}


if __name__ == '__main__':
    print(json.dumps(analyze(sys.argv[1]), indent=2))

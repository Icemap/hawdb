#!/usr/bin/env python3
"""Fixed paired comparison of the unchanged concurrent_writers benchmark."""

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def command_output(command, cwd=None):
    return subprocess.check_output(command, cwd=cwd, text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['baseline', 'candidate', 'baseline-source', 'candidate-source',
                 'fixture-parent', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    args = parser.parse_args()
    source = args.candidate_source.resolve(strict=True)
    baseline = args.baseline_source.resolve(strict=True)
    binaries = {name: getattr(args, name).resolve(strict=True)
                for name in ['baseline', 'candidate']}
    harness = 'benches/concurrent_writers.rs'
    assert digest(source / harness) == digest(baseline / harness)
    assert digest(source / 'Cargo.lock') == digest(baseline / 'Cargo.lock')
    assert digest(source / '.cargo/config.toml') == digest(baseline / '.cargo/config.toml')
    args.fixture_parent.mkdir(parents=True, exist_ok=True)
    filesystem = command_output(['df', '-T', str(args.fixture_parent)])
    assert 'tmpfs' not in filesystem, 'durable fixtures must be disk-backed'
    args.output.mkdir(parents=True, exist_ok=False)
    names = command_output(['git', 'ls-files'], source).splitlines()
    manifest = {name: digest(source / name) for name in names
                if Path(name).suffix in {'.rs', '.toml', '.lock'}}
    (args.output / 'source.json').write_text(json.dumps(manifest, indent=2) + '\n')
    patch = subprocess.check_output(['git', 'diff', 'HEAD', '--', '*.rs'], cwd=source)
    (args.output / 'source.patch').write_bytes(patch)
    protocol = {
        'created_utc': datetime.now(timezone.utc).isoformat(),
        'baseline_revision': 'be0ddb52cce608fb38dc2c5fad38f6182103a336',
        'candidate_parent': command_output(['git', 'rev-parse', 'HEAD'], source),
        'candidate_status': command_output(['git', 'status', '--porcelain'], source),
        'source_manifest_sha256': digest(args.output / 'source.json'),
        'source_patch_sha256': digest(args.output / 'source.patch'),
        'harness_sha256': digest(source / harness),
        'cargo_lock_sha256': digest(source / 'Cargo.lock'),
        'cargo_config_sha256': digest(source / '.cargo/config.toml'),
        'binary_sha256': {name: digest(path) for name, path in binaries.items()},
        'build': 'cargo build --locked --profile bench --bench concurrent_writers; default features',
        'rustc': command_output(['rustc', '-Vv'], source),
        'platform': platform.platform(), 'filesystem': filesystem,
        'cpu': command_output(['lscpu']),
        'pairs_by_writers': {'1': 40, '4': 5, '8': 5},
        'warmup_pairs_by_writers': 1,
        'order': 'alternate binary order and --round 0/1, retain every sample',
        'single_writer_intervals': 'paired-median order statistics [12,29] of 40; six primary p50/p95 endpoints',
        'p99': 'nearest-rank p99 derived from unchanged raw commit samples; descriptive only',
        'limitations': 'shared host; no affinity, cache flush or exclusive machine; 4/8-writer results descriptive',
    }
    (args.output / 'protocol.json').write_text(json.dumps(protocol, indent=2) + '\n')
    environment = dict(os.environ, TMPDIR=str(args.fixture_parent.resolve()))
    measured = {}
    with (args.output / 'results.jsonl').open('x') as raw:
        for writers, pairs in [(1, 40), (4, 5), (8, 5)]:
            for pair in range(-1, pairs):
                order = ['baseline', 'candidate'] if pair < 0 or pair % 2 == 0 else ['candidate', 'baseline']
                for revision in order:
                    command = [str(binaries[revision]), '--writers', str(writers),
                               '--round', str(max(pair, 0) % 2)]
                    started = time.monotonic()
                    load = os.getloadavg()
                    result = subprocess.run(command, text=True, capture_output=True, env=environment)
                    record = {'writers': writers, 'pair': pair, 'warmup': pair < 0,
                              'revision': revision, 'command': command,
                              'elapsed_seconds': time.monotonic() - started,
                              'load_before': load, 'load_after': os.getloadavg(),
                              'returncode': result.returncode, 'stderr': result.stderr}
                    try:
                        assert result.returncode == 0
                        rows = [json.loads(line) for line in result.stdout.splitlines()]
                        assert len(rows) == 6 and all(not row['smoke'] for row in rows)
                        for row in rows:
                            case = row['result']
                            assert case['writers'] == writers
                            assert case['commits'] == case['verified_rows'] == case['commit_latency']['count'] == 256
                            assert len(case['commit_latency']['samples_ns']) == 256
                            assert case['reopen_verified'] == case['durable']
                        assert len({(r['result']['durable'], r['result']['grouped'], r['result']['serialized_control']) for r in rows}) == 6
                    except (AssertionError, ValueError, KeyError):
                        record['stdout'] = result.stdout
                        (args.output / 'failure.json').write_text(json.dumps(record, indent=2) + '\n')
                        raise
                    record['cases'] = rows
                    raw.write(json.dumps(record, separators=(',', ':')) + '\n')
                    raw.flush()
                    if pair >= 0:
                        measured[writers, pair, revision] = rows
                    print(f'writers={writers} pair={pair} revision={revision} seconds={record["elapsed_seconds"]:.3f}', flush=True)
    endpoints = []
    for writers, pairs in [(1, 40), (4, 5), (8, 5)]:
        for durable, grouped in [(False, False), (True, False), (True, True)]:
            for metric in ['p50_ns', 'p95_ns', 'p99_ns', 'commits_per_second']:
                samples = {name: [] for name in binaries}
                for pair in range(pairs):
                    for revision in samples:
                        case = next(row['result'] for row in measured[writers, pair, revision]
                                    if row['result']['durable'] == durable
                                    and row['result']['grouped'] == grouped
                                    and not row['result']['serialized_control'])
                        if metric == 'p99_ns':
                            value = sorted(case['commit_latency']['samples_ns'])[math.ceil(256 * .99) - 1]
                        elif metric == 'commits_per_second':
                            value = case[metric]
                        else:
                            value = case['commit_latency'][metric]
                        samples[revision].append(value)
                ratios = [c / b for b, c in zip(samples['baseline'], samples['candidate'])]
                endpoint = {'writers': writers, 'durable': durable, 'grouped': grouped,
                            'metric': metric, 'median_ratio': statistics.median(ratios),
                            'baseline_median': statistics.median(samples['baseline']),
                            'candidate_median': statistics.median(samples['candidate']),
                            'paired_ratios': ratios}
                if writers == 1 and metric in ['p50_ns', 'p95_ns']:
                    ordered = sorted(ratios)
                    endpoint['ratio_interval'] = [ordered[11], ordered[28]]
                endpoints.append(endpoint)
    receipt = {'measured_cases': 600, 'measured_commits': 153600,
               'measured_reopens': 400, 'endpoints': endpoints,
               'all_six_single_writer_upper_ratios_at_most_one': all(
                   endpoint['ratio_interval'][1] <= 1 for endpoint in endpoints
                   if 'ratio_interval' in endpoint)}
    (args.output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')


if __name__ == '__main__':
    main()

"""Execute the predeclared instrumented diagnostic, never an acceptance gate."""
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import time

q = Path('/evidence')
protocol = json.loads((q / 'protocol.json').read_text())
manifest = json.loads((q / 'source-manifest.json').read_text())
for source_label, files in manifest.items():
    for relative, expected in files.items():
        assert hashlib.sha256((q / ('source-' + source_label) / relative).read_bytes()).hexdigest() == expected, (source_label, relative)

def selected(label):
    return protocol['accepted_build_labels'][label]

for label in protocol['revisions']:
    binary = q / f'{selected(label)}-diagnostic'
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == json.loads((q / f'binary-{selected(label)}.json').read_text())['sha256']
output = q / 'results.jsonl'
assert not output.exists(), 'retain existing measurements; never overwrite or retry a cohort'
fixture = Path('/fixtures/small-projection-vector-ablation-v1')
fixture.mkdir(exist_ok=False)
metadata = {'architecture': platform.machine(), 'platform': platform.platform(), 'started_unix': time.time(), 'protocol_sha256': hashlib.sha256((q / 'protocol.json').read_bytes()).hexdigest()}
(q / 'measurement.json').write_text(json.dumps(metadata, indent=2) + '\n')
counts = {'processes': 0, 'cases': 0, 'commits': 0, 'reads': 0, 'reopens': 0}
with output.open('x') as log:
    for cohort in protocol['cohorts']:
        for pair in range(protocol['pairs']):
            order = ['baseline', 'candidate'] if pair % 2 == 0 else ['candidate', 'baseline']
            for label in order:
                command = [str(q / f'{selected(label)}-diagnostic'), '--writers', str(cohort['writers']), '--readers', str(cohort['readers']), '--round', '0', '--fixture-parent', str(fixture)]
                if cohort['reopen']:
                    command.append('--reopen-before-measure')
                load_before = Path('/proc/loadavg').read_text().strip()
                started = time.monotonic()
                result = subprocess.run(command, capture_output=True, text=True)
                record = {'cohort': cohort, 'pair': pair, 'revision': label, 'command': command, 'returncode': result.returncode, 'seconds': time.monotonic() - started, 'stderr': result.stderr, 'stdout': result.stdout, 'load_before': load_before, 'load_after': Path('/proc/loadavg').read_text().strip()}
                log.write(json.dumps(record) + '\n'); log.flush()
                assert result.returncode == 0, record
                cases = [json.loads(line) for line in result.stdout.splitlines()]
                assert len(cases) == 6
                for row in cases:
                    assert row['schema'] == 'concurrent-writers-query-stages-diagnostic-v1' and not row['smoke']
                    case = row['result']
                    assert case['commits'] == case['verified_rows'] == 256
                    assert case['reopen_verified'] == case['durable']
                    assert case['read_workload']['pinned_snapshot_verified']
                    assert case['read_workload']['requests'] == cohort['readers'] * 64
                    for worker in case['read_workload']['workers']:
                        phases = [worker[name]['samples_ns'] for name in ['capture_latency', 'query_latency', 'drop_latency']]
                        total = worker['latency']['samples_ns']
                        assert all(len(values) == 64 for values in phases + [total])
                        assert all(sum(parts) == elapsed for parts, elapsed in zip(zip(*phases), total))
                        stage_samples = worker['query_stage_samples_ns']
                        assert len(stage_samples) == 64
                        assert all(len(stages) == 3 and all(value > 0 for value in stages) for stages in stage_samples)
                        assert all(sum(stages) <= elapsed for stages, elapsed in zip(stage_samples, worker['query_latency']['samples_ns']))
                        assert all(a <= b for a, b in zip(worker['epochs'], worker['epochs'][1:]))
                    counts['cases'] += 1
                    counts['commits'] += 256
                    counts['reads'] += case['read_workload']['requests']
                    counts['reopens'] += case['durable']
                counts['processes'] += 1
                print(json.dumps({'cohort': cohort, 'pair': pair, 'revision': label, 'seconds': record['seconds']}), flush=True)
assert counts == {'processes': 32, 'cases': 192, 'commits': 49152, 'reads': 110592, 'reopens': 128}, counts
(q / 'receipt.json').write_text(json.dumps({'diagnostic_only': True, 'exact_checks_passed': True, 'counts': counts, 'results_sha256': hashlib.sha256(output.read_bytes()).hexdigest()}, indent=2) + '\n')

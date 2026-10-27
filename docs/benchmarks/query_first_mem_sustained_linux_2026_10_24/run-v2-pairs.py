"""Paired sustained-reader measurements, with source and result validation."""
import argparse
import hashlib
import itertools
import json
import os
import resource
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).parent
parser = argparse.ArgumentParser()
parser.add_argument('--pilot', action='store_true')
args = parser.parse_args()
receipts = {}
for side, mode in itertools.product(('baseline', 'candidate'), ('latency', 'instrumented')):
    name = f'{side}-v2-{mode}'
    receipt = json.loads((ROOT / f'{name}-build-receipt.json').read_text())
    assert receipt['exit_code'] == 0
    binary = ROOT / 'bin' / name
    assert hashlib.sha256(binary.read_bytes()).hexdigest() == receipt['binary_sha256']
    receipts[(side, mode)] = receipt
assert len({r['benchmark_sha256'] for r in receipts.values()}) == 1
for mode in ('latency', 'instrumented'):
    assert receipts[('baseline', mode)]['binary_sha256'] != receipts[('candidate', mode)]['binary_sha256']
rounds = 1 if args.pilot else 3
run = ROOT / (('v2-pilot-' if args.pilot else 'v2-paired-') + str(time.time_ns()))
run.mkdir()
(run / 'protocol.json').write_text(json.dumps({
    'protocol': 'hawdb-mem-query-concurrency-paired-driver-v2',
    'rounds': rounds, 'pilot': args.pilot, 'process_timeout_seconds': 600,
    'expected_processes': 48 * rounds, 'builds': receipts and {
        f'{side}-{mode}': r for (side, mode), r in receipts.items()},
}, indent=2) + '\n')
def host_memory():
    wanted = {'MemAvailable', 'SwapFree', 'Dirty', 'Writeback'}
    result = {}
    for line in Path('/proc/meminfo').read_text().splitlines():
        key, value = line.split(':', 1)
        if key in wanted:
            result[key + '_kib'] = int(value.split()[0])
    for line in Path('/proc/vmstat').read_text().splitlines():
        key, value = line.split()
        if key in {'pgmajfault', 'pswpin', 'pswpout'}:
            result[key] = int(value)
    return result

sequence = 0
for read_kind, out_of_core, writers, pair, mode in itertools.product(
    ('point', 'report', 'stream'), (False, True), (1, 2), range(rounds), ('latency', 'instrumented')
):
    order = ('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')
    for side in order:
        label = f'{sequence:03}-{read_kind}-{int(out_of_core)}-{writers}-{pair}-{mode}-{side}'
        sequence += 1
        job = {
            'path': str(run / (label + '-db')), 'writers': writers, 'readers': 2,
            'writer_iterations': 10 if args.pilot else 1000,
            'reader_iterations': 20 if args.pilot else 4000,
            'rows': 32 if args.pilot else 256, 'read_kind': read_kind,
            'out_of_core': out_of_core, 'sustain_readers': True,
        }
        job_path = run / (label + '-job.json')
        job_path.write_text(json.dumps(job, indent=2) + '\n')
        binary = ROOT / 'bin' / f'{side}-v2-{mode}'
        receipt = {
            'label': label, 'binary_sha256': receipts[(side, mode)]['binary_sha256'],
            'job': job, 'load_before': os.getloadavg(), 'started_unix_ns': time.time_ns(),
            'host_memory_before': host_memory(),
        }
        usage_before = resource.getrusage(resource.RUSAGE_CHILDREN)
        start = time.monotonic()
        raw_path = run / (label + '-raw.json')
        with raw_path.open('w') as output, (run / (label + '-stderr.log')).open('w') as error:
            try:
                status = subprocess.run([str(binary), str(job_path)], stdout=output, stderr=error, timeout=600)
                receipt['exit_code'] = status.returncode
            except subprocess.TimeoutExpired:
                receipt.update(exit_code=None, timed_out=True)
        receipt.update(elapsed_seconds=time.monotonic()-start, load_after=os.getloadavg())
        usage_after = resource.getrusage(resource.RUSAGE_CHILDREN)
        receipt['host_memory_after'] = host_memory()
        receipt['whole_child_usage'] = {key: getattr(usage_after, key) - getattr(usage_before, key)
            for key in ('ru_utime', 'ru_stime', 'ru_minflt', 'ru_majflt', 'ru_inblock', 'ru_oublock')}
        failure = None
        if receipt['exit_code'] == 0:
            try:
                raw = json.loads(raw_path.read_text())
                assert raw['protocol'] == 'hawdb-mem-query-concurrency-v2'
                assert raw['job'] == job and raw['errors'] == 0 and raw['durable_reopen']
                assert raw['successful_writes'] == writers * job['writer_iterations']
                assert raw['reads']['calls'] >= job['readers'] * job['reader_iterations']
                assert raw['instrumented'] == (mode == 'instrumented')
                if mode == 'instrumented':
                    before = raw['instrumentation']['preparation_before']
                    after = raw['instrumentation']['preparation_after']
                    assert all(after[k] >= before[k] for k in before)
                    assert after['cypher_parse_calls'] > before['cypher_parse_calls']
                receipt['validated'] = True
            except (AssertionError, KeyError, ValueError) as error:
                failure = repr(error)
                receipt.update(validated=False, validation_error=failure)
        receipt['raw_sha256'] = hashlib.sha256(raw_path.read_bytes()).hexdigest()
        (run / (label + '-receipt.json')).write_text(json.dumps(receipt, indent=2) + '\n')
        print(json.dumps(receipt), flush=True)
        if receipt['exit_code'] != 0 or failure is not None:
            print(f'FAILED; artifacts retained at {run}', flush=True)
            raise SystemExit(1)
print(f'COMPLETE {run}', flush=True)

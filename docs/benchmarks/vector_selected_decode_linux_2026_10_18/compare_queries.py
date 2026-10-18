#!/usr/bin/env python3
"""Compare frozen public query binaries sequentially against one immutable fixture."""

import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import time


ROOT = Path('/home/hawkingrei/devel/nowledge/skein')
SOURCE = ROOT / '.scratch/issue-994-rabitq-baseline'
ORIGINAL = ROOT / '.scratch/issue-994-rabitq-artifacts/profile-build-1'
OUTPUT = ROOT / '.scratch/issue-1067-vector-artifacts/query-comparison-1'


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def fixture_hashes():
    root = ORIGINAL / 'fixture'
    return {str(path.relative_to(root)): digest(path)
            for path in sorted(root.rglob('*')) if path.is_file()}


def write_record(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def read_event(process, timeout):
    ready, _, _ = select.select([process.stdout], [], [], timeout)
    if not ready:
        raise TimeoutError(f'no query event within {timeout} seconds; pid={process.pid}')
    line = process.stdout.readline()
    if not line:
        raise RuntimeError(f'query process closed stdout; pid={process.pid}')
    return json.loads(line)


def run_one(index, label, binary):
    output = OUTPUT / f'{index}-{label}'
    output.mkdir()
    command = [str(binary), 'profile', str(ORIGINAL / 'fixture')]
    record = {'state': 'started', 'command': command,
              'binary_sha256': digest(binary), 'load_before': os.getloadavg(),
              'started_unix': time.time()}
    write_record(output / 'run.json', record)
    with (output / 'stderr.log').open('w') as error_log:
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=error_log, text=True)
        record['pid'] = process.pid
        write_record(output / 'run.json', record)
        try:
            ready = read_event(process, 120)
            assert ready['phase'] == 'ready', ready
            record['ready'] = ready
            record['state'] = 'measuring'
            write_record(output / 'run.json', record)
            process.stdin.write('run\n')
            process.stdin.flush()
            process.stdin.close()
            complete = read_event(process, 600)
            assert complete['phase'] == 'complete', complete
            assert len(complete['elapsed_ns']) == ready['repetitions'] == 100
            # Popen has not polled or waited. Reap once to retain this child's rusage.
            waited, status, usage = os.wait4(process.pid, 0)
            assert waited == process.pid
            process.returncode = os.waitstatus_to_exitcode(status)
            assert process.returncode == 0, process.returncode
            record.update(state='passed', completed=complete, exit_code=process.returncode,
                          user_cpu_seconds=usage.ru_utime, system_cpu_seconds=usage.ru_stime,
                          max_rss_kib=usage.ru_maxrss, load_after=os.getloadavg(),
                          finished_unix=time.time())
        except Exception as error:
            record.update(state='observation_failed', error=repr(error))
            write_record(output / 'run.json', record)
            # Retain a live handle if observation failed; do not start a replacement.
            raise
        finally:
            write_record(output / 'run.json', record)
    print(json.dumps({'run': index, 'label': label, 'state': record['state'],
                      'max_rss_kib': record['max_rss_kib']}), flush=True)
    return record


OUTPUT.mkdir(parents=True)
source_files = ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                'crates/search/src/out_of_core.rs',
                'crates/search/src/out_of_core/vector_serving.rs',
                'examples/rabitq_query_profile.rs']
source_hashes = {name: digest(SOURCE / name) for name in source_files}
before = fixture_hashes()
write_record(OUTPUT / 'fixture-before.json', before)
metadata = {
    'state': 'running', 'base': '4c32610910cc78771eeb23d729be7107ff769bf6',
    'baseline_helper_commit': '3ae50b58614d8b15e7dc0ca5210e091c070c2e10',
    'source_sha256': source_hashes, 'order': ['original', 'candidate', 'candidate', 'original'],
    'profile': 'repository release plus CARGO_PROFILE_RELEASE_DEBUG=1',
    'features': 'root defaults plus vector-search; explicit scalar kernel',
    'scope': 'Warm-cache repeated public serving query, no sampling profiler. Each process opens the same fixture and performs three warmups before 100 measured queries. Shared host, not idle-host qualified. Peak RSS and CPU cover the complete child process including open/warmup; they are not query-only admission measurements.',
}
write_record(OUTPUT / 'comparison.json', metadata)
binaries = {'original': ORIGINAL / 'rabitq-query-profile',
            'candidate': ROOT / '.scratch/issue-1067-vector-artifacts/rabitq-query-candidate'}
results = []
for index, label in enumerate(metadata['order']):
    results.append(run_one(index, label, binaries[label]))
    expected = {key: value for key, value in results[0]['ready'].items() if key != 'pid'}
    actual = {key: value for key, value in results[-1]['ready'].items() if key != 'pid'}
    assert actual == expected, (actual, expected)
after = fixture_hashes()
write_record(OUTPUT / 'fixture-after.json', after)
assert after == before, 'fixture bytes changed during comparison'
assert source_hashes == {name: digest(SOURCE / name) for name in source_files}
metadata.update(state='passed', runs=results, fixture_files=len(before))
write_record(OUTPUT / 'comparison.json', metadata)
print('All four runs, report parity and fixture hashes passed.', flush=True)

#!/usr/bin/env python3
"""Compare frozen public query binaries sequentially against one immutable fixture."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import select
import subprocess
import time


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--original', type=Path, required=True)
parser.add_argument('--candidate', type=Path, required=True)
parser.add_argument('--fixture', type=Path, required=True)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--repetitions', type=int, default=10)
args = parser.parse_args()
assert args.repetitions > 0
OUTPUT = args.output


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def fixture_hashes():
    root = args.fixture
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


def run_one(index, label, binary, candidate_limit):
    output = OUTPUT / f'{candidate_limit}-{index}-{label}'
    output.mkdir()
    command = [str(binary), str(args.fixture), str(candidate_limit), str(args.repetitions)]
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
            assert len(complete['elapsed_ns']) == ready['repetitions'] == args.repetitions
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
    print(json.dumps({'run': index, 'label': label, 'candidate_limit': candidate_limit, 'state': record['state'],
                      'max_rss_kib': record['max_rss_kib']}), flush=True)
    return record


OUTPUT.mkdir(parents=True)
binaries = {'original': args.original, 'candidate': args.candidate}
binary_hashes = {key: digest(path) for key, path in binaries.items()}
before = fixture_hashes()
write_record(OUTPUT / 'fixture-before.json', before)
metadata = {
    'state': 'running', 'binary_sha256': binary_hashes,
    'order': ['original', 'candidate', 'candidate', 'original'],
    'candidate_limits': [1024, 50000, 100000],
    'repetitions_per_process': args.repetitions,
    'scope': 'Warm-cache public queries against one immutable fixture. Three warmups per process. Complete Debug output and exact hit score bits are verified outside every timer. Full-process wait4 CPU/RSS include warmups and validation; these are not query-only costs.',
}
write_record(OUTPUT / 'comparison.json', metadata)
results = []
for candidate_limit in metadata['candidate_limits']:
    expected = None
    for index, label in enumerate(metadata['order']):
        result = run_one(index, label, binaries[label], candidate_limit)
        actual = {key: value for key, value in result['ready'].items() if key != 'pid'}
        if expected is None:
            expected = actual
        assert actual == expected, f'full report mismatch for candidate limit {candidate_limit}'
        results.append(result)
        metadata['runs'] = results
        write_record(OUTPUT / 'comparison.json', metadata)
after = fixture_hashes()
write_record(OUTPUT / 'fixture-after.json', after)
assert after == before, 'fixture bytes changed during comparison'
assert binary_hashes == {key: digest(path) for key, path in binaries.items()}
metadata.update(state='passed', fixture_files=len(before))
write_record(OUTPUT / 'comparison.json', metadata)
print('All twelve runs, complete output parity and fixture hashes passed.', flush=True)

"""Read-only alternating measurements after compilation and fuzz have completed."""
import hashlib
import json
import platform
import subprocess
import time
from pathlib import Path

ROOT = Path('/private/tmp/hawdb-ordered-seek-qualification')
BINARY = ROOT / 'composite-ordered-final'
POINT_BASELINE = ROOT / 'point-baseline-v1'
POINT_CANDIDATE = ROOT / 'point-candidate-final'


def run(arguments, output_path):
    with output_path.open('w') as output, output_path.with_suffix('.err').open('w') as error:
        result = subprocess.run(arguments, stdout=output, stderr=error, check=False)
    if result.returncode:
        raise RuntimeError((arguments, result.returncode, output_path.with_suffix('.err').read_text()))
    return [json.loads(line) for line in output_path.read_text().splitlines()]


def main():
    assert BINARY.exists() and POINT_BASELINE.exists() and POINT_CANDIDATE.exists()
    # An active compiler or HawDB campaign invalidates this run before timing.
    processes = subprocess.check_output(['ps', '-Ao', 'pid,comm'], text=True)
    busy = [line for line in processes.splitlines() if any(name in line for name in ('/rustc', 'hawdb-mvcc-fuzz', 'hawdb_storage_', 'shiro_'))]
    if busy:
        raise RuntimeError(('compilation/campaign still active', busy))
    binary_manifest = json.loads((ROOT / 'binaries-final.json').read_text())
    actual_head = subprocess.check_output(['git', '-C', '/private/tmp/hawdb-optimizer-work', 'rev-parse', 'HEAD'], text=True).strip()
    assert binary_manifest['source'] == actual_head
    assert not subprocess.check_output(['git', '-C', '/private/tmp/hawdb-optimizer-work', 'status', '--porcelain'], text=True)
    metadata = {
        'started_unix': time.time(), 'platform': platform.platform(),
        'candidate_source': actual_head,
        'baseline_source': 'bb7d213cd114b69d248397dd93a093b0ad035841',
        'binary_hashes': {str(path.name): hashlib.sha256(path.read_bytes()).hexdigest() for path in (BINARY, POINT_BASELINE, POINT_CANDIDATE)},
        'profile': 'repository release, pinned Rust 1.97.1; no overrides',
        'cache_policy': 'first sample labelled warmup and retained; Materialized fixture preparation then OutOfCore reopen, exact count preflight; no device-cache flush',
        'measurement_notes': 'No CPU affinity, hardware exclusivity, or device-cold claim. Index bytes exclude canonical hydration. Preserve every sample including regressions.',
    }
    assert metadata['binary_hashes'] == binary_manifest['sha256']
    (ROOT / 'measurement-final-protocol.json').write_text(json.dumps(metadata, indent=2) + '\n')
    for size in (64, 1000, 10000, 100000):
        for persistent in (False, True):
            path = str(ROOT / f'measured-final-{size}') if persistent else '-'
            if persistent and Path(path).exists():
                raise RuntimeError(('fresh fixture required', path))
            output = ROOT / f'measured-final-{size}-{"disk" if persistent else "memory"}.jsonl'
            rows = run([str(BINARY), str(size), path, '7'], output)
            assert len(rows) == 48
            assert sum(not row['warmup'] for row in rows) == 42
            print('PASS', output.name, flush=True)
    with (ROOT / 'point-pairs-final.jsonl').open('w') as output:
        for pair in range(5):
            for composite in ((False, True) if pair % 2 == 0 else (True, False)):
                for kind in (('baseline', 'candidate') if pair % 2 == 0 else ('candidate', 'baseline')):
                    executable = POINT_BASELINE if kind == 'baseline' else POINT_CANDIDATE
                    samples = run([str(executable)] + (['composite'] if composite else []), ROOT / f'point-final-{kind}-pair{pair}-{composite}.jsonl')
                    assert len(samples) == 11
                    output.write(json.dumps({'pair': pair, 'kind': kind, 'composite': composite, 'samples': samples}) + '\n')
                    output.flush()
    print('PASS point controls: 20 fresh processes, 220 samples', flush=True)


if __name__ == '__main__':
    main()

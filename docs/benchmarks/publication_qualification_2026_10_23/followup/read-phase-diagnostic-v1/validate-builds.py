import hashlib
import json
from pathlib import Path
import subprocess

q = Path('/private/tmp/hawdb-publication-qualification/read-phase-diagnostic-v1')
protocol = json.loads((q / 'protocol.json').read_text())
accepted = {}
for revision, label in protocol['accepted_build_labels'].items():
    name = 'hawdb-1119-read-phase-' + ('baseline-isolated' if revision == 'baseline' else 'candidate') + '-build-1'
    container = json.loads(subprocess.check_output(['docker', 'inspect', name]))[0]
    assert container['State']['Status'] == 'exited' and container['State']['ExitCode'] == 0, name
    artifacts = [json.loads(line) for line in (q / f'build-{label}.jsonl').read_text().splitlines()]
    rebuilt = [row for row in artifacts if row.get('reason') == 'compiler-artifact' and row['target']['name'] in ['hawdb_storage', 'hawdb', 'concurrent_writers']]
    assert {row['target']['name'] for row in rebuilt} == {'hawdb_storage', 'hawdb', 'concurrent_writers'}
    assert all(not row['fresh'] for row in rebuilt), rebuilt
    metadata = json.loads((q / f'binary-{label}.json').read_text())
    assert hashlib.sha256((q / f'{label}-diagnostic').read_bytes()).hexdigest() == metadata['sha256']
    accepted[revision] = {'container': name, 'build_state': container['State'], 'metadata': metadata, 'rebuilt_targets': [row['target'] for row in rebuilt], 'target_directory': next(item for item in container['Config']['Env'] if item.startswith('CARGO_TARGET_DIR='))}
assert accepted['baseline']['metadata']['sha256'] != accepted['candidate']['metadata']['sha256']
assert accepted['baseline']['target_directory'] != accepted['candidate']['target_directory']
(q / 'accepted-builds.json').write_text(json.dumps(accepted, indent=2) + '\n')
print('Both accepted builds independently rebuilt affected packages; hashes differ and source manifests are retained.')

import hashlib,json,subprocess
from pathlib import Path

q=Path('/private/tmp/hawdb-publication-qualification/small-projection-vector-ablation-v1')
previous=Path('/private/tmp/hawdb-publication-qualification/read-query-stages-diagnostic-v1')
protocol=json.loads((q/'protocol.json').read_text())
manifest=json.loads((q/'source-manifest.json').read_text())
for label, files in manifest.items():
    for relative, expected in files.items():
        assert hashlib.sha256((q/('source-'+label)/relative).read_bytes()).hexdigest()==expected,(label,relative)
for name,expected in protocol['harness_sha256'].items():
    for label in manifest:
        assert manifest[label][name]==expected
for label in ['baseline','candidate']:
    assert hashlib.sha256((q/f'{label}-diagnostic').read_bytes()).hexdigest()==json.loads((q/f'binary-{label}.json').read_text())['sha256']
assert (q/'baseline-diagnostic').read_bytes()==(previous/'candidate-diagnostic').read_bytes()
for relative in protocol['candidate_production_overlay_sha256']:
    if relative.endswith('/tests.rs'):
        continue
    assert (q/'source-baseline'/relative).read_bytes()==(previous/'source-candidate'/relative).read_bytes()
    assert manifest['candidate'][relative]==protocol['candidate_production_overlay_sha256'][relative]
container=json.loads(subprocess.check_output(['docker','inspect','hawdb-1119-small-projection-vector-build-1']))[0]
assert container['State']['Status']=='exited' and container['State']['ExitCode']==0
records=[json.loads(line) for line in (q/'build-candidate.jsonl').read_text().splitlines()]
rebuilt=[r for r in records if r.get('reason')=='compiler-artifact' and r['target']['name'] in ['hawdb_executor','hawdb','concurrent_writers']]
assert {r['target']['name'] for r in rebuilt}=={'hawdb_executor','hawdb','concurrent_writers'}
assert all(not r['fresh'] for r in rebuilt)
hashes={label:json.loads((q/f'binary-{label}.json').read_text())['sha256'] for label in ['baseline','candidate']}
assert hashes['baseline']!=hashes['candidate']
(q/'accepted-builds.json').write_text(json.dumps({'binary_sha256':hashes,'control':json.loads((q/'control-provenance.json').read_text()),'candidate_container':container['Name'],'candidate_state':container['State'],'rebuilt_targets':[r['target'] for r in rebuilt],'source_scope':'identical 823e implementation and query-stage harness; only frozen executor projection overlay differs'},indent=2)+'\n')
print('Ablation build/source/binary mapping verified; affected executor and facade targets rebuilt.')

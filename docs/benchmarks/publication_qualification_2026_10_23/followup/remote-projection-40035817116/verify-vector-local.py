"""Verify actual frozen-source native and MVCC campaign outputs."""
from pathlib import Path
import hashlib
import json
import re
import subprocess

q = Path('/private/tmp/hawdb-publication-qualification')
root = Path('/private/tmp/hawdb-optimizer-work')
source = json.loads((q / 'vector-candidate-source.json').read_text())
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip() == source['head']
for name, expected in source['source_overlay_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == expected, name
bep = q / 'vector-candidate-fuzz.bep.json'
records = [json.loads(line) for line in bep.read_text().splitlines()]
summaries = {row['id']['testSummary']['label']: row['testSummary'] for row in records if 'testSummary' in row}
assert len(summaries) == 98 and sum('testSummary' in row for row in records) == 98
assert all(row['overallStatus'] == 'PASSED' and row['totalRunCount'] == 1 for row in summaries.values())
assert any(row.get('finished', {}).get('overallSuccess') for row in records)
assert any(row.get('lastMessage') for row in records)
assert not any('aborted' in row for row in records)
cached = sum(row.get('totalNumCached', 0) for row in summaries.values())
assert all(row.get('totalNumCached', 0) in [0, 1] for row in summaries.values())
log = (q / 'vector-candidate-fuzz.log').read_text()
elapsed = re.findall(r'Elapsed time: ([0-9.]+)s', log)
assert len(elapsed) == 1
mvcc_log = (q / 'vector-candidate-mvcc.log').read_text()
passes = re.findall(r'^PASS \| (\d+) cases \| (\d+) ms \| (.+)$', mvcc_log, re.MULTILINE)
assert len(passes) == 1
cases, milliseconds, directory = passes[0]
assert int(cases) > 0 and int(milliseconds) >= 30000
coverage = re.findall(r'^  (\w+)\s+T1 first:\s+(\d+)\s+T2 first:\s+(\d+)$', mvcc_log, re.MULTILINE)
assert {row[0] for row in coverage} == {'same_key', 'disjoint', 'net_zero', 'absent_delete', 'delete_insert', 'broad_narrow', 'duplicate_insert', 'rollback'}
assert len(coverage) == 8 and all(int(a) > 0 and int(b) > 0 for _, a, b in coverage)
assert sum(int(a) + int(b) for _, a, b in coverage) == int(cases)
compiled = re.findall(r'Finished `dev` profile .* in ([0-9.]+)s', mvcc_log)
assert len(compiled) == 1
report = {'source': source, 'bazel': {'targets': 98, 'passed': 98, 'executed': 98-cached, 'cached': cached, 'elapsed_seconds': float(elapsed[0]), 'bep_sha256': hashlib.sha256(bep.read_bytes()).hexdigest()}, 'mvcc': {'cases': int(cases), 'elapsed_ms': int(milliseconds), 'artifact_directory': directory, 'compile_seconds': float(compiled[0]), 'coverage': [{'kind': kind, 't1_first': int(a), 't2_first': int(b)} for kind, a, b in coverage]}, 'ordinary_new_unit_execution': 'CI owned; not claimed from local fuzz', 'timing_gate_accepted': False}
(q / 'vector-candidate-local-receipt.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))

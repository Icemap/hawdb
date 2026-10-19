"""Audit original select refusals without copying phase diagnoses across hosts."""
from pathlib import Path
import hashlib
import json
import time

B = Path('/private/tmp/hawdb-wip-ready-20261015')
root = B / 'final-github-matrix-collection-0c298984/ordinary'
raw_summary = (root / 'summary.json').read_bytes()
s = json.loads(raw_summary)
assert s['engine'] == '0c2989842321d10e91f136f3c924badb07038add'
audits = []
for bundle in s['reports']:
    for g in bundle['groups']:
        if (g['case'], g['size']) != ('select', 100000) or g['qualified']:
            continue
        raw = (root / f"artifact-{bundle['artifact']}.json").read_bytes()
        a = json.loads(raw)
        assert len(a['verified_uploaded_producers_and_manifests']) == 12
        before, after = (a['reports'][side + '/report.json'] for side in ['baseline', 'candidate'])
        assert before['source']['head'] == '3cdb2610ba8b984676b80ba22df64821787886d3'
        assert after['source']['head'] == s['source']
        assert not before['all_paths_succeeded'] and after['all_paths_succeeded']
        for r in [before, after]:
            assert r['samples'] == 3 and not r['defaults_changed'] and len(r['records']) == 4
            assert sum(not x['discarded'] for x in r['records']) == 3
        errors = []
        for x in before['records']:
            assert x['status'] == 'error'
            assert '16777860 bytes, exceeding its 16777216-byte budget' in x['error']
            errors.append(x['error'])
        for x in after['records']:
            assert x['status'] == 'ok' and x['output_rows'] == 100000
            assert x['durability'] == 'SyncOnEveryWrite'
        phase = 'query warmup' if all(e.startswith('warmup:') for e in errors) else 'not separately labelled by producer'
        audits.append({'selection': [g['case'], g['layer'], g['backend'], g['size']],
                       'artifact': a['artifact'], 'bundle_sha256': hashlib.sha256(raw).hexdigest(),
                       'baseline_errors': errors, 'baseline_error_phase': phase,
                       'baseline_failed_child_attempts': 4, 'candidate_measured_successes': 3,
                       'candidate_discarded_success': 1, 'paired_ratio': None})
assert len({tuple(a['selection']) for a in audits}) == len(audits)
out = {'engine': s['engine'], 'source': s['source'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'audited_groups': len(audits),
       'expected_groups': 6, 'groups': audits,
       'scope': 'Original100k select refusals, source/producer-verified actual errors. '
                'No phase inferred for hosts without a phase label, no usable failed-side latency '
                'or paired ratio. Default/workload/durability unchanged; full90/110 and independent review remain required.'}
(B / '986-0c298984-ordinary-100k-select-refusals.json').write_text(json.dumps(out, indent=2) + '\n')
print('Original select refusal groups audited:', len(audits), 'of6')

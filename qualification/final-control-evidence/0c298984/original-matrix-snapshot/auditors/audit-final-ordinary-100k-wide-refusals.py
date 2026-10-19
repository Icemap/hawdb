"""Preserve both-side wide-result budget refusals, never as timing samples."""
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
        if (g['case'], g['size']) != ('wide', 100000):
            continue
        assert not g['qualified']
        raw = (root / f"artifact-{bundle['artifact']}.json").read_bytes()
        a = json.loads(raw)
        assert len(a['verified_uploaded_producers_and_manifests']) == 12
        sides = {}
        for side, would, head in [
            ('baseline', 16780548, '3cdb2610ba8b984676b80ba22df64821787886d3'),
            ('candidate', 16780218, s['source'])
        ]:
            r = a['reports'][side + '/report.json']
            assert r['source']['head'] == head and not r['all_paths_succeeded']
            assert r['samples'] == 3 and not r['defaults_changed'] and len(r['records']) == 4
            assert sum(not x['discarded'] for x in r['records']) == 3
            errors = []
            for x in r['records']:
                assert x['status'] == 'error' and x['process_exit'] == 0
                assert f'{would} bytes, exceeding its 16777216-byte budget' in x['error']
                assert 'result_materialization' in x['error']
                errors.append(x['error'])
            sides[side] = {'source': r['source'], 'errors': errors,
                           'failed_child_attempts': 4, 'would_use_bytes': would,
                           'budget_bytes': 16777216, 'measured_latency_available': False,
                           'error_phase': 'query warmup' if all(e.startswith('warmup:') for e in errors)
                           else 'not separately labelled by producer'}
        audits.append({'selection': [g['case'], g['layer'], g['backend'], g['size']],
                       'artifact': a['artifact'], 'bundle_sha256': hashlib.sha256(raw).hexdigest(),
                       'sides': sides, 'paired_ratio': None})
assert len({tuple(a['selection']) for a in audits}) == len(audits)
out = {'engine': s['engine'], 'source': s['source'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'audited_groups': len(audits),
       'expected_groups': 6, 'groups': audits,
       'scope': 'Original wide100k groups, both baseline and candidate independently refuse16MiB '
                'result materialization. No usable timing, row-limit diagnosis or success inferred '
                'from child exit0. Host error phases preserved exactly; no copied warmup diagnosis '
                'for unlabelled hosts. Default/workload/sample/deadline unchanged; full90/110 and review remain open.'}
(B / '986-0c298984-ordinary-100k-wide-both-refusals.json').write_text(json.dumps(out, indent=2) + '\n')
print('Original wide100k both-side refusal groups audited:', len(audits), 'of6')

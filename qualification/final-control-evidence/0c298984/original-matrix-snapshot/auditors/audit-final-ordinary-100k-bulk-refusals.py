"""Audit original final-source bulk reports without inventing paired ratios."""
from pathlib import Path
import hashlib
import json
import time

B = Path('/private/tmp/hawdb-wip-ready-20261015')
root = B / 'final-github-matrix-collection-0c298984/ordinary'
raw_summary = (root / 'summary.json').read_bytes()
summary = json.loads(raw_summary)
assert summary['engine'] == '0c2989842321d10e91f136f3c924badb07038add'
rows = []
for bundle in summary['reports']:
    for group in bundle['groups']:
        if (group['size'], group['case'], group['layer']) not in [
            (100000, 'fill_bulk', 'rust'), (100000, 'fill_bulk', 'python'),
            (100000, 'fill_bulk', 'go')
        ]:
            continue
        raw = (root / f"artifact-{bundle['artifact']}.json").read_bytes()
        reports = json.loads(raw)['reports']
        assert bundle['verified_uploaded_producers_and_manifests'] == 12
        before, after = (reports[f'{side}/report.json'] for side in ('baseline', 'candidate'))
        assert before['source']['head'] == '3cdb2610ba8b984676b80ba22df64821787886d3'
        assert after['source']['head'] == summary['source']
        assert not group['qualified'] and not before['all_paths_succeeded']
        assert after['all_paths_succeeded']
        for report in (before, after):
            assert report['samples'] == 3 and report['discarded_iterations'] == 1
            assert not report['defaults_changed'] and len(report['records']) == 4
            assert sum(not r['discarded'] for r in report['records']) == 3
        for record in before['records']:
            assert record['status'] == 'error'
            assert '16777860 bytes, exceeding its 16777216-byte budget' in record['error']
        for record in after['records']:
            assert record['status'] == 'ok' and record['output_rows'] == 100000
            assert record['durability'] == 'SyncOnEveryWrite'
        rows.append({
            'selection': [group['case'], group['layer'], group['backend'], group['size']],
            'artifact': bundle['artifact'], 'bundle_sha256': hashlib.sha256(raw).hexdigest(),
            'baseline_error': before['records'][0]['error'],
            'baseline_measured_refusals': 3, 'candidate_measured_successes': 3,
            'warmups_per_side': 1, 'paired_ratio': None,
        })
assert len({tuple(r['selection']) for r in rows}) == len(rows)
out = {
    'engine': summary['engine'], 'source': summary['source'], 'observed_at': time.time(),
    'summary_sha256': hashlib.sha256(raw_summary).hexdigest(),
    'audited_groups': len(rows), 'groups': rows,
    'scope': 'Original 100k Rust/Python/Go bulk groups: baseline independent16MiB result refusals, '
             'candidate successes; complete producer verification, no paired ratio. '
             'Workload/default/durability unchanged; original full90/110 still required.',
}
(B / '986-0c298984-ordinary-100k-bulk-refusals.json').write_text(json.dumps(out, indent=2) + '\n')
print('Audited original 100k bulk refusal groups:', len(rows))

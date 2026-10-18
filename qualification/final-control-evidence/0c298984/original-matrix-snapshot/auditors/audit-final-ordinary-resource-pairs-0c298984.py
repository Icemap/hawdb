"""Audit complete original ordinary resource pairs, including adverse values."""
from pathlib import Path
import hashlib
import json
import statistics
import time

B = Path('/private/tmp/hawdb-wip-ready-20261015')
root = B / 'final-github-matrix-collection-0c298984/resources'
raw_summary = (root / 'summary.json').read_bytes()
s = json.loads(raw_summary)
assert s['engine'] == '0c2989842321d10e91f136f3c924badb07038add'
audits = []
for group in s['reports']:
    if group['group'][0] != 'ordinary':
        continue
    assert group['verified_files'] == 12
    raw = (root / f"artifact-{group['artifact']}.json").read_bytes()
    reports = json.loads(raw)['reports']
    values = {}
    for side in ['baseline', 'candidate']:
        report = reports[side + '/report.json']
        assert report['all_paths_succeeded'] and report['samples'] == 3
        assert len(report['records']) == 4
        records = [x for x in report['records'] if not x['discarded']]
        assert len(records) == 3
        for x in report['records']:
            assert x['status'] == 'ok' and x['parity'] and x['profile_valid']
            assert x['durability'] == 'SyncOnEveryWrite'
            assert x['output_rows'] == group['group'][1]
        observed = group['sides'][side]['observed_medians']
        medians = {key: statistics.median(x[key] for x in records)
                   for key in ['process_peak_rss_bytes', 'query_boundary_ns', 'consumer_ns']}
        medians['native_profile'] = {
            key: statistics.median(x['native_profile'][key] for x in records)
            for key in observed['native_profile']}
        assert medians == observed
        values[side] = medians
    before, after = values['baseline'], values['candidate']
    audits.append({
        'group': group['group'], 'artifact': group['artifact'],
        'bundle_sha256': hashlib.sha256(raw).hexdigest(), 'observed_medians': values,
        'process_RSS_relative_increase': after['process_peak_rss_bytes'] / before['process_peak_rss_bytes'] - 1,
    })
out = {'engine': s['engine'], 'source': s['source'], 'run': s['run'],
       'observed_at': time.time(), 'summary_sha256': hashlib.sha256(raw_summary).hexdigest(),
       'audited_pairs': len(audits), 'pairs': audits,
       'scope': 'Original instrumented resource pairs only. Three measured processes plus one discarded '
                'per side. All reported allocation/conversion/traced-heap/RSS medians recomputed. '
                'Native requested traffic excludes foreign heaps; whole-process RSS includes setup/runtime/'
                'consumer. Query boundary and consumer timings differ from complete operation. '
                'No pure conversion attribution, significance, causality, hard memory bound or full90/110 acceptance.'}
(B / '986-0c298984-original-ordinary-resource-pairs-audit.json').write_text(json.dumps(out, indent=2) + '\n')
print('Original ordinary resource pairs audited:', len(audits))
for a in audits:
    print(a['group'], 'whole-process RSS relative change', a['process_RSS_relative_increase'])

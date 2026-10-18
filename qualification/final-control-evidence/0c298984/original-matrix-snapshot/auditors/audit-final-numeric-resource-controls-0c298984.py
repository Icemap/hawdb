"""Recompute all original numeric resource controls from complete report bundles."""
from pathlib import Path
import hashlib
import itertools
import json
import math
import statistics
import time

B = Path('/private/tmp/hawdb-wip-ready-20261015')
root = B / 'final-github-matrix-collection-0c298984/resources'
raw_summary = (root / 'summary.json').read_bytes()
s = json.loads(raw_summary)
assert s['engine'] == '0c2989842321d10e91f136f3c924badb07038add'
expected = set(itertools.product(
    [(1000, 'default'), (100000, 'default'), (1000000, 'default'),
     (1000000, 'large'), (1000001, 'large')], ['memory', 'file'], ['false', 'true']))
seen, audits = set(), []
for g in s['reports']:
    if g['group'][0] != 'numeric':
        continue
    _, rows, backend, config, instrumented, case, layer = g['group']
    key = ((rows, config), backend, instrumented)
    assert key in expected and key not in seen
    seen.add(key)
    assert case == 'select' and layer == 'python' and g['verified_files'] == 11
    raw = (root / f"artifact-{g['artifact']}.json").read_bytes()
    a = json.loads(raw)['reports']
    manifest = a['manifest.json']
    assert manifest['engine_source'] == s['engine'] and manifest['source']['head'] == s['source']
    assert manifest['rows'] == rows and manifest['samples'] == 31
    outcome = 'positive' if rows <= 100000 or (rows == 1000000 and config == 'large') else (
        'default_memory_refusal' if config == 'default' else 'explicit_row_refusal')
    for mode in ['owned', 'retained', 'arrow']:
        r = a[mode + '.stdout']
        assert len(r['records']) == 31 and [x['sample'] for x in r['records']] == list(range(31))
        assert r['actual_limits'][0] == (0 if config == 'default' else 1000000)
        assert r['all_paths_succeeded'] == (outcome == 'positive')
        assert g['modes'][mode]['acceptance_failure'] is None
        for x in [r['warmup']] + r['records']:
            if outcome == 'positive':
                assert x['status'] == 'ok' and x['rows'] == rows and x['parity']
            else:
                assert x['status'] == 'error'
                msg = x['error']
                if outcome == 'explicit_row_refusal':
                    assert ('read query returned more than 1000000 rows, exceeding max_read_result_rows 1000000' in msg
                            if mode == 'owned' else 'ResultBudget { rows: 1000001, payload_bytes: 24000024 }' in msg)
                else:
                    assert ('result_materialization' in msg and '16777216-byte budget' in msg
                            if mode == 'owned' else 'retained_source (external_read)' in msg and '268435456-byte budget' in msg)
            if x.get('retained_snapshot'):
                assert all(x['retained_snapshot'][k] == 0 for k in ['retained_bytes', 'buffer_owners', 'view_handles'])
            if 'sample' in x:
                assert x['arrow_pool_bytes_before'] == x['arrow_pool_bytes_after']
        latency = g['modes'][mode]['observed_latency']
        if outcome == 'positive':
            ns = sorted(x['query_boundary_ns'] for x in r['records'])
            assert latency['median_ns'] == statistics.median(ns)
            assert latency['nearest_rank_p95_ns'] == ns[math.ceil(.95 * len(ns)) - 1]
            assert latency['maximum_ns'] == ns[-1]
        else:
            assert latency is None
    audits.append({'group': g['group'], 'artifact': g['artifact'], 'outcome': outcome,
                   'bundle_sha256': hashlib.sha256(raw).hexdigest(), 'measured_attempts': 93, 'warmups': 3})
assert seen == expected
counts = {k: sum(r['outcome'] == k for r in audits)
          for k in ['positive', 'default_memory_refusal', 'explicit_row_refusal']}
assert counts == {'positive': 12, 'default_memory_refusal': 4, 'explicit_row_refusal': 4}
out = {'engine': s['engine'], 'source': s['source'], 'run': s['run'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'groups': audits, 'counts': counts,
       'measured_attempts': 1860, 'warmups': 60,
       'scope': 'Original numeric20 resource controls only. Full producer verification, exact errors, '
                'parity/release/pool balance and observed quantiles recomputed. Plain/instrumented separate; '
                'query boundary differs from complete operation. No whole-memory bound, production tails, '
                'successful refusal ratio, complete ordinary90/resource110 acceptance or independent approval.'}
(B / '986-0c298984-numeric20-resource-controls-complete.json').write_text(json.dumps(out, indent=2) + '\n')
print('All20 original numeric resource controls audited:', counts, '1860 measured attempts +60 warmups')

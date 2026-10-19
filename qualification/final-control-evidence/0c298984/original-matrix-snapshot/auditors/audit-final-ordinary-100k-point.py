"""Recompute final-source original point observations without widening timing scope."""
from pathlib import Path
import hashlib
import json
import math
import statistics
import time

B = Path('/private/tmp/hawdb-wip-ready-20261015')
root = B / 'final-github-matrix-collection-0c298984/ordinary'
raw_summary = (root / 'summary.json').read_bytes()
s = json.loads(raw_summary)
assert s['engine'] == '0c2989842321d10e91f136f3c924badb07038add'
audits = []
for bundle in s['reports']:
    for g in bundle['groups']:
        if (g['case'], g['size']) != ('point', 100000):
            continue
        assert g['qualified']
        raw = (root / f"artifact-{bundle['artifact']}.json").read_bytes()
        a = json.loads(raw)
        assert len(a['verified_uploaded_producers_and_manifests']) == 12
        medians, checksums = {}, set()
        for side, head in [('baseline', '3cdb2610ba8b984676b80ba22df64821787886d3'),
                           ('candidate', s['source'])]:
            r = a['reports'][side + '/report.json']
            assert r['source']['head'] == head and r['all_paths_succeeded']
            assert r['samples'] == 3 and not r['defaults_changed'] and r['seed'] == 24301
            assert len(r['records']) == 4
            xs = [x for x in r['records'] if not x['discarded']]
            assert len(xs) == 3
            for x in r['records']:
                assert x['status'] == 'ok' and x['parity'] and x['phase_timing_valid']
                assert x['output_rows'] == 100000 and x['values'] == 300000 and x['prefetch'] == 0
                assert x['durability'] == 'SyncOnEveryWrite' and x['checksum'] == x['expected_checksum']
                checksums.add(x['checksum'])
            medians[side] = {key: statistics.median(x[key] for x in xs)
                             for key in ['elapsed_ns', 'query_boundary_ns', 'consumer_ns',
                                         'setup_ns', 'process_seconds', 'process_peak_rss_bytes']}
        assert len(checksums) == 1
        for key in ['elapsed_ns', 'query_boundary_ns']:
            assert math.isclose(medians['baseline'][key] / medians['candidate'][key],
                                g[key + '_median_speedup'], rel_tol=1e-12)
        audits.append({'selection': [g['layer'], g['backend']], 'artifact': a['artifact'],
                       'bundle_sha256': hashlib.sha256(raw).hexdigest(), 'measured_medians': medians,
                       'elapsed_median_ratio': g['elapsed_ns_median_speedup'],
                       'query_boundary_median_ratio': g['query_boundary_ns_median_speedup'],
                       'checksums': sorted(checksums)})
assert len({tuple(r['selection']) for r in audits}) == len(audits)
out = {'engine': s['engine'], 'source': s['source'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'audited_groups': len(audits),
       'expected_groups': 6, 'groups': audits,
       'scope': 'Original100k point groups,100000returned rows/300000values,seed24301,prefetch0,'
                'SyncOnEveryWrite,three measured independent processes+discardedwarmup per side. '
                'Query boundary excludes identical checksum consumer; seeded reads use warm stores. '
                'Workload elapsed excludes full child lifetime including fixture/setup/warmup; '
                'process_seconds and RSS are separate. No significance,causality,production tails or full90/110 acceptance.'}
(B / '986-0c298984-ordinary-100k-point-qualified-audit.json').write_text(json.dumps(out, indent=2) + '\n')
print('Original100k point groups audited:', len(audits), 'of6')

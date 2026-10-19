"""Recompute final-source original point observations without widening timing scope."""
from pathlib import Path
import hashlib
import json
import math
import statistics
import subprocess
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
        prefetch_evidence = {}
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
                assert x['output_rows'] == 100000 and x['values'] == 300000
                if g['layer'] == 'go':
                    assert 'prefetch' not in x
                else:
                    assert x['prefetch'] == 0
                assert x['durability'] == 'SyncOnEveryWrite' and x['checksum'] == x['expected_checksum']
                checksums.add(x['checksum'])
            if g['layer'] == 'go':
                source = subprocess.check_output(
                    ['git', 'show', r['source']['tree'] + ':bindings/go/cmd/boundary/main.go'],
                    cwd=(B / 'baseline-profile-fixed-controls/baseline' if side == 'baseline'
                         else B / 'review-disposition-41db'))
                text = source.decode()
                assert 'calls = len(j.Rows)' in text and 'text = j.Point' in text
                assert 'output, err := db.Query(text, params)' in text
                prefetch_evidence[side] = {
                    'reported_value': None, 'reason': 'ordinary Go producer omits prefetch field',
                    'source_tree': r['source']['tree'], 'producer_source_sha256': hashlib.sha256(source).hexdigest(),
                    'call_shape': 'one eager DB.Query per point row, full same checksum consumer',
                    'scope': 'source shape and successful parity; missing field is not inferred as zero'}
            else:
                prefetch_evidence[side] = {'reported_value': 0}
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
                       'checksums': sorted(checksums), 'prefetch_evidence': prefetch_evidence})
assert len({tuple(r['selection']) for r in audits}) == len(audits)
out = {'engine': s['engine'], 'source': s['source'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'audited_groups': len(audits),
       'expected_groups': 6, 'groups': audits,
       'scope': 'Original100k point groups,100000returned rows/300000values,seed24301; Rust/Python report prefetch0, Go omits that field,'
                'SyncOnEveryWrite,three measured independent processes+discardedwarmup per side. '
                'Query boundary excludes identical checksum consumer; seeded reads use warm stores. '
                'Workload elapsed excludes full child lifetime including fixture/setup/warmup; '
                'process_seconds and RSS are separate. No significance,causality,production tails or full90/110 acceptance.'}
(B / '986-0c298984-ordinary-100k-point-qualified-audit.json').write_text(json.dumps(out, indent=2) + '\n')
print('Original100k point groups audited:', len(audits), 'of6')

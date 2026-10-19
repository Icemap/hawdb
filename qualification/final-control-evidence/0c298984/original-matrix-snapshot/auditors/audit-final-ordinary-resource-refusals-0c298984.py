"""Audit resource error windows separately from successful query windows."""
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
    if group['group'][0] != 'ordinary' or all(group['sides'][side]['success'] for side in ['baseline', 'candidate']):
        continue
    assert group['verified_files'] == 12
    raw = (root / f"artifact-{group['artifact']}.json").read_bytes()
    a = json.loads(raw)
    sides = {}
    for side, head in [('baseline', '3cdb2610ba8b984676b80ba22df64821787886d3'),
                       ('candidate', s['engine'])]:
        r = a['reports'][side + '/report.json']
        assert r['source']['head'] == head
        assert r['samples'] == 3 and r['discarded_iterations'] == 1 and not r['defaults_changed']
        assert len(r['records']) == 4
        measured = [x for x in r['records'] if not x['discarded']]
        assert len(measured) == 3
        if r['all_paths_succeeded']:
            for x in r['records']:
                assert x['status'] == 'ok' and x['parity'] and x['profile_valid'] and x['phase_timing_valid']
                assert x['output_rows'] == group['group'][1] and x['durability'] == 'SyncOnEveryWrite'
            observed = group['sides'][side]['observed_medians']
            medians = {key: statistics.median(x[key] for x in measured)
                       for key in observed if key != 'native_profile'}
            medians['native_profile'] = {
                key: statistics.median(x['native_profile'][key] for x in measured)
                for key in observed['native_profile']}
            assert medians == observed
            sides[side] = {'success': True, 'observed_medians': medians,
                           'scope': 'successful query window; whole-process RSS separately includes setup/runtime/consumer'}
        else:
            assert not group['sides'][side]['success'] and group['sides'][side]['observed_medians'] == {}
            for x in r['records']:
                assert x['status'] == 'error' and x['failure_profile_valid']
                assert x['failure_profile_scope'] == 'producer_run_including_setup_warmup_and_cleanup'
                assert 'result_materialization' in x['error'] and '16777216-byte budget' in x['error']
                assert x.get('phase_timing_valid') is None and x.get('parity') is None
                assert 'query_boundary_ns' not in x
            native = measured[0]['failure_native_profile']
            keys = [key for key, value in native.items() if type(value) in [int, float]]
            medians = {key: statistics.median(x['failure_native_profile'][key] for x in measured) for key in keys}
            sides[side] = {'success': False, 'errors': [x['error'] for x in r['records']],
                           'reported_error_types': sorted({x['error_type'] for x in r['records'] if 'error_type' in x}),
                           'reported_error_phases': sorted({x['phase'] for x in r['records'] if 'phase' in x}),
                           'failed_child_attempts': 4, 'measured_failure_native_medians': medians,
                           'failure_python_traced_fields_present': all(
                               'python_traced_current_bytes' in x['failure_native_profile']
                               and 'python_traced_peak_bytes' in x['failure_native_profile'] for x in r['records']),
                           'failure_process_peak_RSS_median': statistics.median(x['process_peak_rss_bytes'] for x in measured),
                           'scope': 'producer_run_including_setup_warmup_and_cleanup',
                           'usable_query_latency': False}
        if group['group'][-1] == 'go':
            failed = not r['all_paths_succeeded']
            key = 'failure_go_host_profile' if failed else 'go_host_profile'
            valid_key = 'failure_go_host_profile_valid' if failed else 'go_host_profile_valid'
            for x in r['records']:
                assert x[valid_key] and x[key]['valid']
                assert x[key]['scope'] == 'go_query_parameter_native_call_result_phases'
                if failed:
                    assert x['failure_go_host_profile_scope'] == 'db_query_calls_including_setup_warmup_and_cleanup'
            counter_keys = [k for k, v in measured[0][key].items() if type(v) in [int, float]]
            sides[side]['observed_go_host_phase_medians'] = {
                k: statistics.median(x[key][k] for x in measured) for k in counter_keys}
            sides[side]['go_host_phase_scope'] = (
                'db_query_calls_including_setup_warmup_and_cleanup' if failed else 'successful query window')
            sides[side]['go_heap_allocation_fields_present'] = all(
                'go_allocated_bytes' in x and 'go_allocations' in x for x in r['records'])
    audits.append({'group': group['group'], 'artifact': group['artifact'],
                   'bundle_sha256': hashlib.sha256(raw).hexdigest(), 'sides': sides,
                   'paired_query_or_allocation_ratio': None})
assert len({a['artifact'] for a in audits}) == len(audits)
out = {'engine': s['engine'], 'source': s['source'], 'observed_at': time.time(),
       'summary_sha256': hashlib.sha256(raw_summary).hexdigest(), 'audited_refusal_groups': len(audits),
       'groups': audits,
       'scope': 'Original ordinary resource refusal/error paths. Error allocation windows include setup/warmup/'
                'cleanup; successful native windows cover query work. No cross-scope allocation ratio, '
                'usable refusal latency, causal, leak-absence, memory-bound or full90/110 acceptance claim.'}
(B / '986-0c298984-original-ordinary-resource-refusals-audit.json').write_text(json.dumps(out, indent=2) + '\n')
print('Ordinary resource refusal groups audited:', len(audits))

from pathlib import Path
import json,hashlib,statistics
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-python-failure-heap-0c298984'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
m=json.loads((O/'source-manifest.json').read_text());d=json.loads((O/'receipt.json').read_text());source=json.loads((O/'source-qualified-audit.json').read_text());assert source['verified']
assert d['source_manifest_sha256']==sha(O/'source-manifest.json')==source['source_manifest_sha256']
for f,h in m['files'].items():assert sha(O/f)==h
out={'engine':m['production_head'],'receipt_sha256':sha(O/'receipt.json'),'source_audit_sha256':sha(O/'source-qualified-audit.json'),'terminal':d['terminal'],'expected_samples_including_discarded':16,'observed_samples':len(d['samples']),'groups':[],'scope':'Supplemental local observer only; original 90/110 counts unchanged. Failure Python tracing begins after setup/warmup and ends before cleanup; native failure window includes full producer. No cross-scope performance ratio or leak claim.'}
for side in ['baseline','candidate']:
 for backend in ['memory','file']:
  samples=[s for s in d['samples'] if s['side']==side and f'-{backend}-' in s['label']]
  if not samples:continue
  for s in samples:
   job=O/(s['label']+'.job.json');assert sha(job)==s['job_sha256'];j=json.loads(job.read_text());r=s['result'];assert len(j['rows'])==100000 and j['case']=='fill_bulk' and j['backend']==backend and j['seed']==24301
   if side=='baseline':
    assert r['status']=='error' and r['phase']=='execute' and r['error_type']=='ExecutionError'
    assert '16777860' in r['error'] and '16777216' in r['error']
    h=r['failure_python_traced_heap'];assert h['available'] and h['scope']=='query_tracing_window_before_error_cleanup';assert h['python_traced_peak_bytes']>=h['python_traced_current_bytes']>0
    assert r['failure_profile_scope']=='producer_run_including_setup_warmup_and_cleanup'
   else:
    assert r['status']=='ok' and r['checksum']==j['expected_checksum'] and r['output_rows']==100000 and r['values']==300000
    assert r['durability']=='SyncOnEveryWrite' and r['prefetch']==0 and 'failure_python_traced_heap' not in r
  measured=[s for s in samples if not s['discarded_warmup']];g={'side':side,'backend':backend,'observed_attempts':len(samples),'measured_samples':len(measured),'complete':len(samples)==4}
  if measured:
   profiles=[s['result']['failure_python_traced_heap'] if side=='baseline' else s['result']['native_profile'] for s in measured]
   g['observed_heap_medians']={key:statistics.median(p[key] for p in profiles) for key in ['python_traced_current_bytes','python_traced_peak_bytes']}
   g['scope']='query_tracing_window_before_error_cleanup' if side=='baseline' else 'successful_query_tracing_window'
  out['groups'].append(g)
if d['terminal']:assert len(d['samples'])==16 and all(g['complete'] for g in out['groups']) and len(out['groups'])==4
(O/'sample-qualified-audit.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out,indent=2))

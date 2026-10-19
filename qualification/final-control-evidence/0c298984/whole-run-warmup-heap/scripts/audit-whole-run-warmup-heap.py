from pathlib import Path
import hashlib,json,statistics,sys
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'whole-run-warmup-heap-0c298984';W=B/'qualify-resources-final-0c298984';sys.path.insert(0,str(W/'bindings/benchmarks'));import run as driver
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();m=json.loads((O/'source-manifest.json').read_text());d=json.loads((O/'receipt.json').read_text());assert d['baseline_head']=='3cdb2610ba8b984676b80ba22df64821787886d3' and d['source_manifest_sha256']==sha(O/'source-manifest.json')
for f,h in m['files'].items():assert sha(O/f)==h
assert d['go_binary_sha256']==sha(O/'boundary-whole-heap') and d['python_interpreter_sha256']==sha((B/'resource-qualification/venv/bin/python').resolve())
for x in d['probes']+d['samples']:
 job=O/(x['label']+'.job.json');assert sha(job)==x['job_sha256'];j=json.loads(job.read_text());r=x['result'];assert r==json.loads((O/(x['label']+'.stdout')).read_text());layer=x['layer'];h=r['whole_run_go_heap' if layer=='go' else 'whole_run_python_heap'];assert h['available'] and h['scope']==m['scope']
 if layer=='go':assert type(h['go_allocated_bytes']) is int and type(h['go_allocations']) is int and h['go_allocated_bytes']>0 and h['go_allocations']>0
 else:assert h['python_traced_peak_bytes']>=h['python_traced_current_bytes']>=0 and h['python_traced_peak_bytes']>0
 if r['status']=='ok':assert r['output_rows']==len(j['rows']) and r['checksum']==j['expected_checksum'] and driver.native_profile_valid(r,layer,True)
 else:
  assert driver.failure_profile_valid(r,layer,True) and '16777216' in r['error'] and 'query_boundary_ns' not in r
  if layer=='go':assert driver.failure_go_host_profile_valid(r,True)
for x in d['samples']:
 j=json.loads((O/(x['label']+'.job.json')).read_text());r=x['result'];assert len(j['rows'])==100000 and j['seed']==24301 and j['case']=='select' and j['backend']==x['backend'];assert r['status']=='error' and r.get('phase',r['error'].split(':',1)[0])=='warmup'
 if x['layer']=='go':
  assert r['failure_go_heap_profile']['available'] is False
  assert r['failure_go_host_profile']['ffi_calls']==198
out={'baseline_head':m['baseline_head'],'receipt_sha256':sha(O/'receipt.json'),'source_manifest_sha256':sha(O/'source-manifest.json'),'terminal':d['terminal'],'probes':len(d['probes']),'observed_attempts':len(d['samples']),'expected_attempts':16,'groups':[],'scope':m['scope'],'limitations':m['limits'],'original_matrix_counts_unchanged':True,'paired_ratio':None}
for layer in ['go','python']:
 for backend in ['memory','file']:
  xs=[x for x in d['samples'] if x['layer']==layer and x['backend']==backend];measured=[x for x in xs if not x['discarded_warmup']];g={'layer':layer,'backend':backend,'attempts':len(xs),'measured':len(measured),'complete':len(xs)==4}
  if measured:
   key='whole_run_go_heap' if layer=='go' else 'whole_run_python_heap';fields=['go_allocations','go_allocated_bytes'] if layer=='go' else ['python_traced_current_bytes','python_traced_peak_bytes'];g['observed_medians']={f:statistics.median(x['result'][key][f] for x in measured) for f in fields}
  out['groups'].append(g)
if d['terminal']:assert len(d['samples'])==16 and len(d['probes'])==6 and all(g['complete'] for g in out['groups'])
(O/'sample-qualified-audit.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out,indent=2))

from pathlib import Path
import json,hashlib,statistics,subprocess,sys
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-go-failure-heap-0c298984';sys.path.insert(0,str(B/'qualify-resources-final-0c298984/bindings/benchmarks'));import run as driver
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();m=json.loads((O/'build-source-manifest.json').read_text());d=json.loads((O/'receipt.json').read_text())
assert m['baseline_head']=='3cdb2610ba8b984676b80ba22df64821787886d3'
assert d['build_source_manifest_sha256']==sha(O/'build-source-manifest.json') and d['binary_sha256']==sha(O/'boundary-failure-heap')
assert m['native_library_sha256']==sha(O/'baseline-ffi.dylib')
for f,h in m['files'].items():assert sha(O/f)==h
for item in d['probes']+d['samples']:
 job=O/(item['label']+'.job.json');assert sha(job)==item['job_sha256'];j=json.loads(job.read_text());r=item['result'];assert r==json.loads((O/(item['label']+'.stdout')).read_text())
 assert r['go_version']=='go1.27.2' and r['pointer_bits']==64
 if r['status']=='ok':
  assert r['output_rows']==len(j['rows']) and r['checksum']==j['expected_checksum'] and 'failure_go_heap_profile' not in r
  assert driver.native_profile_valid(r,'go',True) and driver.go_host_profile_valid(dict(r,size=len(j['rows'])),True)
 else:
  assert driver.failure_profile_valid(r,'go',True) and driver.failure_go_host_profile_valid(r,True)
  h=r['failure_go_heap_profile'];assert h['scope']=='query_allocation_window_after_error_cleanup' and h['available']==r['error'].startswith('execute:')
  if h['available']:assert h['go_allocations']>0 and h['go_allocated_bytes']>0
  else:assert 'go_allocations' not in h and 'go_allocated_bytes' not in h
  assert 'query_boundary_ns' not in r
for item in d['samples']:
 j=json.loads((O/(item['label']+'.job.json')).read_text());r=item['result'];assert len(j['rows'])==100000 and j['case']=='fill_bulk' and j['seed']==24301
 assert r['status']=='error' and r['error'].startswith('execute:') and '16777860' in r['error'] and '16777216' in r['error']
out={'baseline_head':m['baseline_head'],'go_source_tree':m['go_source_tree'],'receipt_sha256':sha(O/'receipt.json'),'manifest_sha256':sha(O/'build-source-manifest.json'),'terminal':d['terminal'],'observed_attempts':len(d['samples']),'expected_attempts':8,'probes':len(d['probes']),'groups':[],'build_metadata':subprocess.check_output(['go','version','-m',str(O/'boundary-failure-heap')],text=True),'scope':m['scope'],'original_matrix_counts_unchanged':True}
for backend in ['memory','file']:
 records=[s for s in d['samples'] if f'-{backend}-' in s['label']];measured=[s for s in records if not s['discarded_warmup']];g={'backend':backend,'attempts':len(records),'measured':len(measured),'complete':len(records)==4}
 if measured:g['observed_error_heap_medians']={key:statistics.median(s['result']['failure_go_heap_profile'][key] for s in measured) for key in ['go_allocated_bytes','go_allocations']}
 out['groups'].append(g)
if d['terminal']:assert len(d['samples'])==8 and len(d['probes'])==3 and all(x['complete'] for x in out['groups'])
(O/'sample-qualified-audit.json').write_text(json.dumps(out,indent=2)+'\n');print(json.dumps(out,indent=2))

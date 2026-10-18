"""Audit actual supplemental allocation controls without replacing original failures."""
from pathlib import Path
import hashlib,json,subprocess
from functools import lru_cache
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'qualify-supplemental-file-final-0c298984';F=B/'resource-checkpoint-0c298984-frozen-package';OUT=B/'resource-checkpoint-0c298984-results'
s=json.loads((B/'resource-checkpoint-0c298984-runtime-state.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip()==s['observer_head']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=W,text=True).strip()
for name,sha in s['frozen_sha256'].items():assert hashlib.sha256((F/name).read_bytes()).hexdigest()==sha
assert (F/'numeric.py').read_bytes()==(W/'qualification/resources/numeric.py').read_bytes()
@lru_cache(None)
def oracle(n):
 value=14695981039346656037
 for i in range(n):
  for _ in range(3):value=((value^i)*1099511628211)&((1<<64)-1)
 return {'rows':n,'checksum':f'{value:016x}'}
results=[];errors=[];missing=[]
for n in [1000,1000001]:
 for mode in ['owned','retained','arrow']:
  p=OUT/(str(n)+'-'+mode+'.json')
  if not p.exists() or not p.read_text().strip():missing.append([n,mode]);continue
  row=next((x for x in s['records'] if x['rows']==n and x['mode']==mode),None)
  if row is None or 'report_sha256' not in row:missing.append([n,mode]);continue
  try:
   r=json.loads(p.read_text())
   assert 'exit' in row and hashlib.sha256(p.read_bytes()).hexdigest()==row['report_sha256']
   assert (r['rows'],r['mode'],r['backend'],r['samples'],r['instrumented'],r['pyarrow'])==(n,mode,'file',31,True,'26.0.0')
   assert len(r['records'])==31 and [x['sample'] for x in r['records']]==list(range(31))
   assert 'setup_or_probe_error' not in r and r['fixture_reference']==oracle(n) and r['setup_completed_rows']==n
   limits=r['actual_limits'];expected_success=n==1000
   assert r['configuration']==('default' if expected_success else 'large')
   if expected_success:
    assert limits[:4]==[0,67108864,268435456,16777216] and limits[4]>0 and limits[5:]==[16777216,67108864]
    assert r['setup_checkpoint_rows']==0 and r['setup_checkpoints']==[]
   else:
    assert limits==[1000000,1073741824,1073741824,1073741824,2147483648,268435456,67108864]
    assert r['setup_checkpoint_rows']==65536
    assert [x['completed_rows'] for x in r['setup_checkpoints']]==[65536*i for i in range(1,16)]
    assert all(type(x['elapsed_ns']) is int and x['elapsed_ns']>0 for x in r['setup_checkpoints'])
   assert row['exit']==(0 if expected_success else 1) and r['all_paths_succeeded'] is expected_success
   for x in [r['warmup'],*r['records']]:
    if expected_success:assert x['status']=='ok' and x['parity'] and {'rows':x['rows'],'checksum':x['checksum']}==oracle(n)
    else:
     expected=('execution error: read query returned more than 1000000 rows, exceeding max_read_result_rows 1000000' if mode=='owned' else 'retained query ResultBudget { rows: 1000001, payload_bytes: 24000024 }')
     assert x['status']=='error' and x['error']==expected
    snapshot=x.get('retained_snapshot')
    if mode!='owned':assert snapshot is not None
    if snapshot:assert all(snapshot[k]==0 for k in ['retained_bytes','buffer_owners','view_handles'])
   nonzero=[];disagreements=[];arrow_nonzero=[]
   for x in r['records']:
    profile=x['native_profile'];assert all(type(v) is int and v>=0 for v in profile.values())
    assert type(x['complete_attempt_ns']) is int and x['complete_attempt_ns']>0
    assert x['python_traced_peak_bytes']>=x['python_traced_live_bytes']>=0
    assert min(x['arrow_pool_bytes_before'],x['arrow_pool_bytes_after'])>=0
    delta=profile['live_requested_bytes_after']-profile['live_requested_bytes_before'];net=profile['allocated_bytes']-profile['deallocated_bytes']
    if delta:nonzero.append({'sample':x['sample'],'live_delta':delta})
    if delta!=net:disagreements.append({'sample':x['sample'],'live_delta':delta,'counter_net':net})
    if x['arrow_pool_bytes_before']!=x['arrow_pool_bytes_after']:arrow_nonzero.append(x['sample'])
   latency=r.get('observed_latency')
   if expected_success:
    values=sorted(x['query_boundary_ns'] for x in r['records']);assert (latency['samples'],latency['median_ns'],latency['nearest_rank_p95_ns'],latency['maximum_ns'])==(31,values[15],values[29],values[-1])
   else:assert latency is None
   assert type(r['process_peak_rss_bytes']) is int and r['process_peak_rss_bytes']>0
   results.append({'rows':n,'mode':mode,'query_success':expected_success,'row_boundary_proven':not expected_success,'instrumented_windows':31,'ownership_released':True,'nonzero_native_windows':nonzero,'counter_disagreements':disagreements,'arrow_pool_nonzero_windows':arrow_nonzero,'process_peak_rss_bytes':r['process_peak_rss_bytes'],'observed_instrumented_latency':latency,'report_sha256':row['report_sha256']})
  except Exception as e:errors.append({'rows':n,'mode':mode,'kind':type(e).__name__,'detail':str(e)[:1000]})
result={'engine':s['engine'],'observer_head':s['observer_head'],'native_compiled_source':s['native_compiled_source'],'frozen_sha256':s['frozen_sha256'],'groups':results,'missing':missing,'errors':errors,'complete':len(results)==6 and not errors,'acceptance_approved':False,'scope':'Supplemental source-bound allocation/error control only. Original failed resource/phase groups retained. Process-global allocator windows are not per-query leak proofs; instrumented latency is separate from ordinary latency; RSS includes fixture/checkpoints. No full90/110 acceptance or whole-memory bound.'}
(B/'resource-checkpoint-0c298984-full-audit.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'observed':len(results),'expected':6,'missing':missing,'errors':errors,'row_boundary_modes':[r['mode'] for r in results if r['row_boundary_proven']]}))

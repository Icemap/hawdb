from pathlib import Path
import json,subprocess,hashlib,sys,time,os
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'whole-run-warmup-heap-0c298984';W=B/'qualify-resources-final-0c298984';sys.path.insert(0,str(W/'bindings/benchmarks'));import run as driver
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();m=json.loads((O/'source-manifest.json').read_text());go=O/'boundary-whole-heap';gohash=sha(go);python=B/'resource-qualification/venv/bin/python';pyhash=sha(python.resolve());d={'baseline_head':m['baseline_head'],'source_manifest_sha256':sha(O/'source-manifest.json'),'go_binary_sha256':gohash,'python_interpreter_sha256':pyhash,'scope':m['scope'],'terminal':False,'probes':[],'samples':[],'original_matrix_counts_unchanged':True}
def save():(O/'receipt.json').write_text(json.dumps(d,indent=2)+'\n')
def verify():
 for f,h in m['files'].items():assert sha(O/f)==h,f
 assert sha(go)==gohash and sha(python.resolve())==pyhash

def run(layer,data,label):
 verify();job=O/(label+'.job.json');job.write_text(json.dumps(data,ensure_ascii=False));cmd=[str(go),str(job),str(O/'baseline-ffi.dylib')] if layer=='go' else [str(python),'-B',str(O/'python_boundary.py'),str(job),str(O/'python-baseline/hawdb/_hawdb.so')];env=dict(os.environ,PYTHONPATH=str(O/'python-baseline'),PYTHONDONTWRITEBYTECODE='1');start=time.monotonic();p=subprocess.run(cmd,env=env,capture_output=True,text=True);(O/(label+'.stdout')).write_text(p.stdout);(O/(label+'.stderr')).write_text(p.stderr);assert p.returncode==0,(label,p.stderr);r=json.loads(p.stdout)
 h=r['whole_run_go_heap' if layer=='go' else 'whole_run_python_heap'];assert h['available'] and h['scope']==m['scope']
 if layer=='go':assert h['go_allocations']>0 and h['go_allocated_bytes']>0
 else:assert h['python_traced_peak_bytes']>=h['python_traced_current_bytes']>=0 and h['python_traced_peak_bytes']>0
 if r['status']=='ok':
  assert r['checksum']==data['expected_checksum'] and r['output_rows']==len(data['rows']) and driver.native_profile_valid(r,layer,True)
  if layer=='go':assert driver.go_host_profile_valid(dict(r,size=len(data['rows'])),True)
 else:
  assert driver.failure_profile_valid(r,layer,True) and '16777216' in r['error'] and 'query_boundary_ns' not in r
  if layer=='go':assert driver.failure_go_host_profile_valid(r,True)
 verify();print(label,r['status'],r.get('phase',r.get('error','').split(':',1)[0]),flush=True);return {'label':label,'layer':layer,'job_sha256':sha(job),'process_seconds':time.monotonic()-start,'result':r}
save()
for layer in ['go','python']:
 for mode in ['success','warmup_refusal','execute_refusal']:
  data=driver.fixture(1000 if mode=='success' else 10000,'select' if mode=='success' else 'wide')
  if mode=='execute_refusal':data['case']='fill_bulk'
  data.update(backend='memory',path=str(O/(layer+'-'+mode+'.db')));x=run(layer,data,'probe-'+layer+'-'+mode);d['probes'].append(x);save();assert x['result']['status']==('ok' if mode=='success' else 'error')
  if mode!='success':assert x['result'].get('phase',x['result']['error'].split(':',1)[0])==('warmup' if mode=='warmup_refusal' else 'execute')
for layer in ['go','python']:
 for backend in ['memory','file']:
  for sample in range(4):
   label=f'{layer}-100k-select-{backend}-{sample}';data=driver.fixture(100000,'select');data.update(backend=backend,path=str(O/(label+'.db')));x=run(layer,data,label);x.update(backend=backend,discarded_warmup=sample==0);d['samples'].append(x);save();r=x['result'];assert r['status']=='error' and r.get('phase',r['error'].split(':',1)[0])=='warmup'
d.update(terminal=True,source_and_artifacts_unchanged=True);save();print('complete',flush=True)

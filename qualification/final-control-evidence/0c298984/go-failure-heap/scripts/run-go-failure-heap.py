from pathlib import Path
import json,subprocess,hashlib,sys,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-go-failure-heap-0c298984';sys.path.insert(0,str(B/'qualify-resources-final-0c298984/bindings/benchmarks'));import run as driver
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest();m=json.loads((O/'build-source-manifest.json').read_text());binary=O/'boundary-failure-heap';binary_hash=sha(binary)
d={'status':'functional_probes','terminal':False,'build_source_manifest_sha256':sha(O/'build-source-manifest.json'),'binary_sha256':binary_hash,'probes':[],'samples':[],'scope':m['scope'],'original_matrix_counts_unchanged':True}
def save():(O/'receipt.json').write_text(json.dumps(d,indent=2)+'\n')
def verify():
 assert sha(binary)==binary_hash and sha(O/'baseline-ffi.dylib')==m['native_library_sha256']
 for f,h in m['files'].items():assert sha(O/f)==h,f

def run(data,label):
 verify();job=O/(label+'.job.json');job.write_text(json.dumps(data,ensure_ascii=False));start=time.monotonic();p=subprocess.run([str(binary),str(job),str(O/'baseline-ffi.dylib')],capture_output=True,text=True)
 (O/(label+'.stdout')).write_text(p.stdout);(O/(label+'.stderr')).write_text(p.stderr);assert p.returncode==0,(label,p.stderr);r=json.loads(p.stdout)
 if r['status']=='ok':
  assert r['checksum']==data['expected_checksum'] and r['output_rows']==len(data['rows']) and 'failure_go_heap_profile' not in r
  assert driver.native_profile_valid(r,'go',True) and driver.go_host_profile_valid(dict(r, size=len(data['rows'])), True)
 else:
  assert driver.failure_profile_valid(r,'go',True) and driver.failure_go_host_profile_valid(r,True)
  heap=r['failure_go_heap_profile'];assert heap['available']==r['error'].startswith('execute:')
  if heap['available']:assert heap['go_allocations']>0 and heap['go_allocated_bytes']>0
  else:assert 'go_allocations' not in heap
 verify();print(label,r['status'],flush=True);return {'label':label,'job_sha256':sha(job),'process_seconds':time.monotonic()-start,'result':r}
save()
for mode in ['success','warmup_refusal','execute_refusal']:
 data=driver.fixture(1000 if mode=='success' else 10000,'select' if mode=='success' else 'wide')
 if mode=='execute_refusal':data['case']='fill_bulk'
 data.update(backend='memory',path=str(O/(mode+'.db')));x=run(data,'probe-'+mode);d['probes'].append(x);save();assert x['result']['status']==('ok' if mode=='success' else 'error')
d['status']='waiting_for_python_supplement_to_finish';save();print(d['status'],flush=True)
# Preserve the existing Python process; serialize the additional measurement
# campaign rather than changing the concurrency of the original matrix.
while True:
 p=json.loads((B/'supplemental-python-failure-heap-0c298984/receipt.json').read_text())
 if p['terminal']:break
 process=subprocess.run(['ps','-p','94456','-o','command='],capture_output=True,text=True)
 assert process.returncode==0 and 'run-python-failure-heap.py' in process.stdout,'Python campaign ended without terminal receipt; inspect before proceeding'
 time.sleep(30)
d['status']='sampling';save()
for backend in ['memory','file']:
 for sample in range(4):
  label=f'baseline-100k-bulk-{backend}-{sample}';data=driver.fixture(100000,'fill_bulk');data.update(backend=backend,path=str(O/(label+'.db')));x=run(data,label);x['discarded_warmup']=sample==0;d['samples'].append(x);save();assert x['result']['status']=='error' and '16777216' in x['result']['error']
d.update(status='completed',terminal=True,source_and_artifacts_unchanged=True);save();print('completed',flush=True)

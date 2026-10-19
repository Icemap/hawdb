from pathlib import Path
import sys,os,json,hashlib,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');O=B/'supplemental-python-failure-heap-0c298984';W=B/'qualify-resources-final-0c298984'
sys.path.insert(0,str(W/'bindings/benchmarks'));import run as driver
python=B/'resource-qualification/venv/bin/python'
manifest=json.loads((O/'source-manifest.json').read_text())
def verify():
 for name,digest in manifest['files'].items():assert hashlib.sha256((O/name).read_bytes()).hexdigest()==digest,name
report={'source_manifest_sha256':hashlib.sha256((O/'source-manifest.json').read_bytes()).hexdigest(),'probes':[],'samples':[],'terminal':False,'original_matrix_counts_unchanged':True}
def save(): (O/'receipt.json').write_text(json.dumps(report,indent=2)+'\n')
def execute(side,data,label):
 verify();job=O/(label+'.job.json');job.write_text(json.dumps(data,ensure_ascii=False));env=dict(os.environ,PYTHONPATH=str(O/side),PYTHONDONTWRITEBYTECODE='1')
 start=time.monotonic();p=subprocess.run([str(python),'-B',str(O/'python_boundary.py'),str(job),str(O/side/'hawdb/_hawdb.so')],env=env,capture_output=True,text=True)
 (O/(label+'.stdout')).write_text(p.stdout);(O/(label+'.stderr')).write_text(p.stderr)
 assert p.returncode==0,(label,p.returncode,p.stderr)
 result=json.loads(p.stdout);item={'side':side,'label':label,'job_sha256':hashlib.sha256(job.read_bytes()).hexdigest(),'process_seconds':time.monotonic()-start,'result':result}
 if result['status']=='error':
  assert driver.failure_profile_valid(result,'python',True),(label,result)
  heap=result['failure_python_traced_heap'];assert heap['available']==(result['phase']=='execute')
  if heap['available']:assert heap['python_traced_peak_bytes']>=heap['python_traced_current_bytes']>0
  else:assert 'python_traced_peak_bytes' not in heap
 else:
  assert result['checksum']==data['expected_checksum'] and result['output_rows']==len(data['rows'])
  assert driver.native_profile_valid(result,'python',True)
  assert 'failure_python_traced_heap' not in result
 verify();print(label,result['status'],result.get('phase'),flush=True);return item
save()
for mode in ['success','warmup_refusal','execute_refusal']:
 data=driver.fixture(1000 if mode=='success' else 10000,'select' if mode=='success' else 'wide')
 if mode=='execute_refusal':data['case']='fill_bulk'
 data.update(backend='memory',path=str(O/(mode+'.db')))
 item=execute('baseline',data,'probe-'+mode);report['probes'].append(item);save()
 assert item['result']['status']==('ok' if mode=='success' else 'error')
for backend in ['memory','file']:
 for side in ['baseline','candidate']:
  for sample in range(4):
   label=f'{side}-100k-bulk-{backend}-{sample}';data=driver.fixture(100000,'fill_bulk');data.update(backend=backend,path=str(O/(label+'.db')))
   item=execute(side,data,label);item['discarded_warmup']=sample==0;report['samples'].append(item);save()
   result=item['result']
   if side=='baseline':assert result['status']=='error' and result['phase']=='execute' and '16777216' in result['error']
   else:assert result['status']=='ok'
report['terminal']=True;report['source_and_artifacts_unchanged']=True;verify();save();print('complete',flush=True)

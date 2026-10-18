"""Additional expected-refusal observations; never promote failures to latency samples."""
from pathlib import Path
import argparse,hashlib,importlib.util,json,os,subprocess,sys,tempfile,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');F=B/'phase-prototype-import-only-package'

def load(name,path):
 spec=importlib.util.spec_from_file_location(name,path);module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module);return module

def child(backend,mode,case):
 control=json.loads((B/'phase-prototype-import-control-receipt.json').read_text())
 for name,h in control['frozen_package_sha256'].items():assert hashlib.sha256((F/name).read_bytes()).hexdigest()==h
 sys.path.insert(0,str(F));import hawdb;import hawdb._hawdb as native
 original=load('original_consumer',F/'retained_boundary.py');observer=load('phase_observer',F/'consume.py')
 assert observer.snapshot(native)['allocation_instrumented'] is False
 record={'backend':backend,'mode':mode,'case':case,'rows':1000,'samples':31,'records':[],'extension_sha256':control['extension_sha256'],'consumer_sha256':control['consumer_sha256'],'scope':'Additional private-prototype expected-refusal diagnostics. Original fixture/consumer, default budgets/durability, fresh process per backend/mode/error. No failed query is a successful latency sample; no full matrix or final published source acceptance.'}
 with tempfile.TemporaryDirectory(prefix='hawdb-exception-profile-') as directory:
  db=hawdb.open(None if backend=='memory' else Path(directory)/'db')
  try:
   limits=tuple(db._qualification_limits());assert limits[:4]==(0,67108864,268435456,16777216);record['actual_limits']=limits
   db.execute('CREATE NODE TABLE Delivery');db.execute('CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT')
   for start in range(0,1000,512):db.execute('UNWIND $rows AS row CREATE (:Delivery {score: row.score})',{'rows':[{'score':n} for n in range(start,min(start+512,1000))]})
   expected=original.fixture_reference(1000)
   class Probe:
    def execute_retained(self,query,params):
     kwargs={}
     if case=='strict_source_reuse':kwargs['require_source_reuse']=True
     elif case=='row_ceiling':kwargs.update(max_result_rows=1,batch_rows=1)
     elif case=='unsupported_plan':query='RETURN 1 AS score'
     return db.execute_retained(query,params,options=hawdb.RetainedOptions(**kwargs))
    def retained_snapshot_copy(self):return db.retained_snapshot_copy()
   probe=Probe()
   for i in range(32):
    r=observer.attempt(native,original,probe,mode,expected)
    assert r['status']=='error' and r['phase_control_valid'] and not r['phase_attribution_valid']
    assert r['handoff_boundary_residual_ns'] is None
    phases=r['native_phases'];assert phases['prepare_calls']==1 and phases['prepare_ns']>0
    if case=='row_ceiling':assert 'ResultBudget' in r['error'] and phases['pull_calls']>0 and phases['pull_ns']>0
    else:assert phases['pull_calls']==0 and phases['pull_ns']==0
    if case=='strict_source_reuse':assert 'CopyRequired' in r['error'] or 'copy' in r['error'].lower()
    if case=='unsupported_plan':assert 'UnsupportedPlan' in r['error'] or 'unsupported' in r['error'].lower()
    quota=r.get('retained_snapshot')
    if quota is not None:assert all(quota[k]==0 for k in ['retained_bytes','buffer_owners','view_handles'])
    r['sample']=i-1
    if i==0:record['warmup']=r
    else:record['records'].append(r)
   retry=observer.attempt(native,original,db,mode,expected)
   assert retry['status']=='ok' and retry['parity'] and retry['phase_control_valid'] and retry['phase_attribution_valid']
   assert all(retry['retained_snapshot'][k]==0 for k in ['retained_bytes','buffer_owners','view_handles'])
   record.update(healthy_retry=retry,expected_refusals_observed=True,qualified_latency_samples=0)
  finally:db.close()
 for name,h in control['frozen_package_sha256'].items():assert hashlib.sha256((F/name).read_bytes()).hexdigest()==h
 print(json.dumps(record))

if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--backend',choices=['memory','file']);parser.add_argument('--mode',choices=['retained','arrow']);parser.add_argument('--case',choices=['strict_source_reuse','row_ceiling','unsupported_plan']);args=parser.parse_args()
 if args.backend:child(args.backend,args.mode,args.case)
 else:
  state=B/'retained-exception-resource-probes-state.json';assert not state.exists();out=B/'retained-exception-resource-probes';out.mkdir(exist_ok=False)
  s={'status':'running','pid':os.getpid(),'started':time.time(),'groups':[]}
  def save():
   p=state.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(state)
  try:
   save()
   for backend in ['memory','file']:
    for mode in ['retained','arrow']:
     for case in ['strict_source_reuse','row_ceiling','unsupported_plan']:
      name=f'{backend}-{mode}-{case}';report=out/(name+'.json');error=out/(name+'.stderr');entry={'backend':backend,'mode':mode,'case':case,'report':str(report),'started':time.time()};s['groups'].append(entry);save()
      with report.open('w') as stdout,error.open('w') as stderr:r=subprocess.run([str(B/'resource-qualification/venv/bin/python'),__file__,'--backend',backend,'--mode',mode,'--case',case],stdout=stdout,stderr=stderr,env=dict(os.environ,PYTHONNOUSERSITE='1'))
      entry.update(exit=r.returncode,ended=time.time());save();assert r.returncode==0,(name,error)
      data=json.loads(report.read_text());assert len(data['records'])==31 and data['expected_refusals_observed'];entry['sha256']=hashlib.sha256(report.read_bytes()).hexdigest();save()
   s.update(status='passed_expected_refusal_diagnostics_only',ended=time.time());save()
  except BaseException as error:
   s.update(status='failed',error=repr(error),ended=time.time());save();raise

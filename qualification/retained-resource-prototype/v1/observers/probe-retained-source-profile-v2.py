"""Observe source construction around the unchanged consumer; discard diagnostic timings."""
from pathlib import Path
import hashlib,importlib.util,json,os,subprocess,sys,tempfile,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');F=B/'phase-prototype-import-only-package';STATE=B/'retained-source-profile-probe-v2-state.json';assert not STATE.exists()
control=json.loads((B/'phase-prototype-import-control-receipt.json').read_text())
s={'status':'waiting_for_serial_validation_terminal','pid':os.getpid(),'started':time.time(),'observations':[],'scope':'Source-profile diagnostics on the private frozen assembled prototype, default1000-row memory/file only. Original fixture and complete consumer bytes are unchanged. Read-only profile copies add overhead; all timing fields are discarded. No performance, final-head or full-matrix claim.'}
def save():
 p=STATE.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(STATE)
try:
 save()
 while True:
  q=json.loads((B/'986-serial-validation-orchestrator-v6-state.json').read_text())
  live=subprocess.run(['ps','-p',str(q['pid']),'-o','command='],capture_output=True,text=True)
  if q['status'] in ['passed','failed'] and (live.returncode or 'resume-986-serial-validation-v6.py' not in live.stdout):break
  assert live.returncode==0 and 'resume-986-serial-validation-v6.py' in live.stdout
  time.sleep(10)
 for name,h in control['frozen_package_sha256'].items():assert hashlib.sha256((F/name).read_bytes()).hexdigest()==h
 sys.path.insert(0,str(F));import hawdb
 import hawdb._hawdb as native
 assert Path(native.__file__).resolve()==F/'hawdb/_hawdb.so'
 assert native._retained_phase_snapshot()['allocation_instrumented'] is False
 spec=importlib.util.spec_from_file_location('original_consumer',F/'retained_boundary.py');consumer=importlib.util.module_from_spec(spec);spec.loader.exec_module(consumer)
 class CursorProbe:
  def __init__(self,cursor,record):self.cursor,self.record=cursor,record;self.observe('before_pull')
  def observe(self,phase):
   p=self.cursor.profile_copy();self.record[phase]=dict(p)
   maximum=self.record.setdefault('maximum_observed',{})
   for k,v in p.items():
    if type(v) is int:maximum[k]=max(maximum.get(k,0),v)
  def next_batch(self):
   batch=self.cursor.next_batch();self.observe('after_pull' if batch is not None else 'after_eof');return batch
  def close(self):self.observe('before_close');return self.cursor.close()
 class DatabaseProbe:
  def __init__(self,database,record):self.database,self.record=database,record
  def execute_retained(self,*args,**kwargs):return CursorProbe(self.database.execute_retained(*args,**kwargs),self.record)
  def __getattr__(self,name):return getattr(self.database,name)
 s['status']='probing';save()
 for backend in ['memory','file']:
  with tempfile.TemporaryDirectory(prefix='hawdb-source-profile-') as directory:
   db=hawdb.open(None if backend=='memory' else Path(directory)/'db')
   try:
    limits=tuple(db._qualification_limits());assert limits[:4]==(0,67108864,268435456,16777216)
    db.execute('CREATE NODE TABLE Delivery');db.execute('CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT')
    for start in range(0,1000,512):db.execute('UNWIND $rows AS row CREATE (:Delivery {score: row.score})',{'rows':[{'score':n} for n in range(start,min(start+512,1000))]})
    profile={};result=consumer.attempt(DatabaseProbe(db,profile),'retained',consumer.fixture_reference(1000),{})
    assert result['status']=='ok' and result['parity']
    assert all(result['retained_snapshot'][k]==0 for k in ['retained_bytes','buffer_owners','view_handles'])
    assert profile['before_pull']['visited_rows']==0 and profile['before_pull']['source_constructed_bytes']==0
    assert profile['before_pull']['source_preflight_rows']==1000 and profile['before_pull']['source_pinned_capacity_bytes']>0
    assert profile['after_eof']['visited_rows']==1000 and profile['after_eof']['source_constructed_bytes']==16000
    assert profile['after_eof']['source_pinned_capacity_bytes']==0 and profile['after_eof']['outstanding_batches']==0
    for k in ['query_boundary_ns','elapsed_ns']:result.pop(k,None)
    s['observations'].append({'backend':backend,'actual_limits':limits,'result':result,'profile':profile});save()
   finally:db.close()
 for name,h in control['frozen_package_sha256'].items():assert hashlib.sha256((F/name).read_bytes()).hexdigest()==h
 s.update(status='passed_source_profile_only',ended=time.time(),extension_sha256=control['extension_sha256'],consumer_sha256=control['consumer_sha256']);save()
except BaseException as error:
 s.update(status='failed',error=repr(error),ended=time.time());save();raise

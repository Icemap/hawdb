from pathlib import Path
import hashlib,json,os,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');Q=B/'phase-owned-v2-qualification-state.json';S=B/'phase-owned-v2-error-unwind-state.json';assert not S.exists();s={'status':'waiting_for_serial_callback_controls','pid':os.getpid(),'scope':'One parsing refusal followed by the unchanged successful owned consumer, validating real native timer/error unwinding. Not a performance sample or full exception matrix.'}
def save():S.write_text(json.dumps(s,indent=2)+'\n')
try:
 save()
 while True:
  q=json.loads(Q.read_text())
  if q['status']=='passed_all_three_mode_native_coverage':break
  assert q['status']!='failed','Preceding source qualification failed';os.kill(q['pid'],0);time.sleep(10)
 F=B/'phase-owned-v2-frozen-package';env={k:v for k,v in os.environ.items() if k not in ['PYTHONPATH','PYTHONHOME']};env.update(PYTHONPATH=str(F),PYTHONNOUSERSITE='1',PYTHONDONTWRITEBYTECODE='1')
 code='''import importlib.util,json,hawdb
from pathlib import Path
from hawdb import _hawdb as native
root=Path.cwd()
def load(name,path):
 s=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(s);s.loader.exec_module(m);return m
observer=load("v2_observer",root/"consume.py");consumer=load("original_consumer",root/"retained_boundary.py")
db=hawdb.open()
try:
 db.execute("CREATE NODE TABLE Delivery")
 db.execute("CREATE PROPERTY ON NODE TABLE Delivery(score) TYPE INT")
 db.execute("CREATE (:Delivery {score: 0})")
 before=observer.snapshot(native)
 try:db.execute("MATCH (")
 except hawdb.ParseError as error:rejected=dict(type=type(error).__name__,message=str(error))
 else:raise AssertionError("malformed query unexpectedly succeeded")
 after=observer.snapshot(native)
 assert after["owned_calls"]-before["owned_calls"]==1 and after["owned_ns"]>before["owned_ns"]
 for key in ["prepare_calls","prepare_ns","pull_calls","pull_ns"]:assert after[key]==before[key]
 retry=observer.attempt(native,consumer,db,"owned",consumer.fixture_reference(1))
 assert retry["status"]=="ok" and retry["parity"] and retry["phase_attribution_valid"] and retry["native_phases"]["owned_calls"]==1
 assert not observer.snapshot(native)["overflowed"]
 print(json.dumps(dict(rejected=rejected,error_owned_calls=1,retry=retry,scope="Expected parse failure and complete healthy original consumer; no timing ratio.")))
finally:db.close()
'''
 s['status']='probing';save();r=subprocess.run([str(B/'resource-qualification/venv/bin/python'),'-c',code],cwd=F,env=env,capture_output=True,text=True);(B/'phase-owned-v2-error-unwind.stdout').write_text(r.stdout);(B/'phase-owned-v2-error-unwind.stderr').write_text(r.stderr);assert r.returncode==0,r.stderr
 report=json.loads(r.stdout);s.update(status='passed_error_and_healthy_retry',result=report,ended=time.time());save()
except BaseException as e:s.update(status='failed',error=repr(e),ended=time.time());save();raise

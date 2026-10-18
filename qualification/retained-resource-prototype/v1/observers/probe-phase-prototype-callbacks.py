from pathlib import Path
import hashlib,itertools,json,os,shutil,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'qualify-phase-prototype-current';F=B/'phase-prototype-import-only-package';OUT=B/'phase-prototype-callback-probes';STATE=B/'phase-prototype-callback-probes-state.json'
assert not STATE.exists() and not OUT.exists()
control=json.loads((B/'phase-prototype-import-control-receipt.json').read_text());source=json.loads((B/'phase-prototype-compile-state.json').read_text());assert source['status']=='passed_compile_and_lint_only'
observer=W/'qualification/phases/consume.py';assert hashlib.sha256(observer.read_bytes()).hexdigest()==source['source_sha256']['qualification/phases/consume.py'];shutil.copyfile(observer,F/'consume.py');observer_sha=hashlib.sha256((F/'consume.py').read_bytes()).hexdigest()
s={'status':'waiting_for_serial_validation_terminal','pid':os.getpid(),'started':time.time(),'records':[],'scope':'Private phase-hook coverage probes only: original complete consumer and fixture, fresh mode process, 1000 rows on memory/file, 31 measured attempts plus warmup. No full matrix, ordinary performance, baseline comparison or final-head acceptance claim.'}
def save():
 p=STATE.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(STATE)
def guard():
 assert all(hashlib.sha256((W/p).read_bytes()).hexdigest()==h for p,h in source['source_sha256'].items())
 for p,h in control['frozen_package_sha256'].items():assert hashlib.sha256((F/p).read_bytes()).hexdigest()==h
 assert hashlib.sha256((F/'consume.py').read_bytes()).hexdigest()==observer_sha
try:
 save()
 while True:
  q=json.loads((B/'986-serial-validation-orchestrator-state.json').read_text())
  if q['status'] in ['passed','failed']:
   live=subprocess.run(['ps','-p',str(q['pid']),'-o','command='],capture_output=True,text=True)
   if live.returncode or 'resume-986-serial-validation.py' not in live.stdout:break
  else:
   live=subprocess.run(['ps','-p',str(q['pid']),'-o','command='],capture_output=True,text=True)
   assert live.returncode==0 and 'resume-986-serial-validation.py' in live.stdout,'Serial pipeline vanished without a terminal state'
  time.sleep(10)
 guard();OUT.mkdir();s.update(status='probing',serial_validation_terminal=q['status'],extension_sha256=control['extension_sha256'],observer_sha256=observer_sha,consumer_sha256=control['consumer_sha256']);save()
 for backend,mode in itertools.product(['memory','file'],['owned','retained','arrow']):
  guard();name=backend+'-'+mode;output=OUT/(name+'.json');error=OUT/(name+'.stderr');progress=OUT/(name+'-progress.json')
  cmd=[str(B/'resource-qualification/venv/bin/python'),str(F/'consume.py'),'--package',str(F),'--consumer',str(F/'retained_boundary.py'),'--extension',str(F/'hawdb/_hawdb.so'),'--consumer-sha256',control['consumer_sha256'],'--extension-sha256',control['extension_sha256'],'--rows','1000','--samples','31','--backend',backend,'--mode',mode,'--configuration','default','--progress-report',str(progress)]
  r={'backend':backend,'mode':mode,'command':cmd,'started':time.time(),'output':str(output),'stderr':str(error)};s['records'].append(r);save()
  with output.open('w') as out,error.open('w') as err:result=subprocess.run(cmd,cwd=F,env=dict(os.environ,PYTHONNOUSERSITE='1'),stdout=out,stderr=err)
  r.update(exit=result.returncode,ended=time.time());save();assert result.returncode==0,(backend,mode)
  report=json.loads(output.read_text());assert report['terminal'] and report['all_paths_succeeded'] and len(report['records'])==31
  assert all(x['status']=='ok' and x['parity'] and x['phase_control_valid'] for x in report['records'])
  if mode!='owned':assert all(x['phase_attribution_valid'] and x['native_phases']['prepare_calls']==1 and x['native_phases']['pull_calls']>0 for x in report['records'])
  r['report_sha256']=hashlib.sha256(output.read_bytes()).hexdigest();save()
 guard();s.update(status='passed_callback_coverage_only',ended=time.time());save()
except BaseException as error:
 s.update(status='failed',error=repr(error),ended=time.time());save();raise

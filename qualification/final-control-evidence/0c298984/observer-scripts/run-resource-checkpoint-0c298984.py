"""Separate source-bound allocation/error controls; original matrix remains unchanged."""
from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015')
W=B/'qualify-supplemental-file-final-0c298984'
N=B/'qualify-resources-final-0c298984'
F=B/'resource-checkpoint-0c298984-frozen-package'
OUT=B/'resource-checkpoint-0c298984-results'
STATE=B/'resource-checkpoint-0c298984-runtime-state.json'
PYTHON=B/'resource-qualification/venv/bin/python'
ENGINE='0c2989842321d10e91f136f3c924badb07038add'
def git(root,*args):return subprocess.check_output(['git',*args],cwd=root,text=True).strip()
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert json.loads((B/'986-final-supplemental-0c298984-prepare-state.json').read_text())['status']=='prepared_and_native_built'
assert not git(W,'status','--porcelain') and not git(N,'status','--porcelain')
head=git(W,'rev-parse','HEAD');native_head=git(N,'rev-parse','HEAD')
assert git(W,'merge-base',ENGINE,head)==ENGINE
assert git(W,'rev-parse',ENGINE+'^{tree}')=='122ad7e9bcbe6ccc38e49dd018bed00c4d02c3a3'
assert git(W,'diff','--name-only',native_head,head).splitlines()==['qualification/phases/consume.py','qualification/resources/numeric.py','qualification/resources/numeric_driver.py']
assert git(W,'diff','--name-only',ENGINE,head).splitlines()==json.loads((W/'qualification/resources/campaign.json').read_text())['overlay_files']
assert not F.exists() and not OUT.exists() and not STATE.exists()
F.mkdir();OUT.mkdir()
shutil.copytree(W/'bindings/python/python/hawdb',F/'hawdb',ignore=shutil.ignore_patterns('__pycache__','*.pyc','*.so','*.dylib'))
shutil.copyfile((N/'bazel-bin/bindings/python/lib_hawdb_boundary_profile.dylib').resolve(strict=True),F/'_hawdb.so')
shutil.copyfile(W/'qualification/resources/numeric.py',F/'numeric.py')
shutil.copyfile(W/'bindings/benchmarks/retained_boundary.py',F/'retained_boundary.py')
hashes={str(p.relative_to(F)):digest(p) for p in F.rglob('*') if p.is_file()}
assert hashes['retained_boundary.py']=='a9275fa36bd57feddc463a53b60f386d37bebba4337b4fee87040ba35ae3a9d1'
s={'status':'running','pid':os.getpid(),'started':time.time(),'engine':ENGINE,'observer_head':head,'observer_tree':git(W,'rev-parse','HEAD^{tree}'),'native_compiled_source':native_head,'native_compiled_tree':git(N,'rev-parse','HEAD^{tree}'),'build_command':['bazel','build','-c','opt','//bindings/python:_hawdb_boundary_profile','//bindings/python:_hawdb_retained_phase_profile','--repo_env=CARGO_BAZEL_ISOLATED=false','--repo_env=CARGO_NET_OFFLINE=true'],'frozen_sha256':hashes,'records':[],'scope':'Supplemental fresh file-backed allocation/error controls,31samples plus warmup per mode. Explicit65536row fixture checkpoints only for1M+1; unchanged native inputs relative to qualification bdb03c06 for engine0c298984 and original full consumer. No original matrix replacement, refused latency or whole-memory/leak claim.'}
def save():
 p=STATE.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(STATE)
interpreter=PYTHON.resolve(strict=True);interpreter_sha256=digest(interpreter);s['interpreter']={'path':str(interpreter),'sha256':interpreter_sha256}
def guard():
 assert digest(interpreter)==interpreter_sha256
 assert git(W,'rev-parse','HEAD')==head and not git(W,'status','--porcelain')
 assert git(N,'rev-parse','HEAD')==native_head and not git(N,'status','--porcelain')
 assert all(digest(F/p)==h for p,h in hashes.items())
try:
 save()
 for n,configuration,cadence in [(1000,'default',0),(1000001,'large',65536)]:
  for mode in ['owned','retained','arrow']:
   guard();name=str(n)+'-'+mode
   command=[str(PYTHON),'-B',str(F/'numeric.py'),'--package',str(F),'--extension',str(F/'_hawdb.so'),'--consumer',str(F/'retained_boundary.py'),'--rows',str(n),'--samples','31','--backend','file','--configuration',configuration,'--mode',mode,'--instrumented','--setup-checkpoint-rows',str(cadence)]
   row={'rows':n,'configuration':configuration,'mode':mode,'command':command,'started':time.time()};s['records'].append(row);save()
   env={k:v for k,v in os.environ.items() if k not in ['PYTHONPATH','PYTHONHOME']};env.update(PYTHONNOUSERSITE='1',PYTHONDONTWRITEBYTECODE='1')
   with (OUT/(name+'.json')).open('w') as out,(OUT/(name+'.stderr')).open('w') as err:r=subprocess.run(command,cwd=F,env=env,stdout=out,stderr=err)
   row.update(exit=r.returncode,ended=time.time());save();guard()
   report=json.loads((OUT/(name+'.json')).read_text());row.update(setup_completed_rows=report.get('setup_completed_rows'),setup_checkpoint_count=len(report['setup_checkpoints']),samples=len(report['records']),setup_or_probe_error=report.get('setup_or_probe_error'),report_sha256=digest(OUT/(name+'.json')));save()
 s.update(status='collected',ended=time.time());save()
except BaseException as error:s.update(status='collector_failed',error=type(error).__name__,ended=time.time());save();raise

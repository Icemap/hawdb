from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'review-disposition-41db';ENGINE='0c2989842321d10e91f136f3c924badb07038add';STATE=B/'986-final-held-write-controls-0c298984-state.json';OUT=B/'final-held-write-controls-0c298984-results';assert not STATE.exists() and not OUT.exists();OUT.mkdir()
def git(*a):return subprocess.check_output(['git',*a],cwd=W,text=True).strip()
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
assert git('rev-parse','HEAD')==ENGINE and not git('status','--porcelain');TREE=git('rev-parse','HEAD^{tree}');PYTHON=B/'resource-qualification/venv/bin/python';interpreter=PYTHON.resolve(strict=True);ish=digest(interpreter)
s={'status':'building','pid':os.getpid(),'started':time.time(),'engine':ENGINE,'tree':TREE,'steps':[],'records':[],'scope':'Final-source existing held-node/held-numeric write diagnostics. Exact original consumers, fixtures, rotating rounds and safe durability; same-source control, not new90/110 groups or inter-revision causality.'}
def save():
 p=STATE.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(STATE)
env={k:v for k,v in os.environ.items() if k not in ['PYTHONPATH','PYTHONHOME']};env.update(PYTHONNOUSERSITE='1',PYTHONDONTWRITEBYTECODE='1',CARGO_TARGET_DIR=str(B/'pr986/target'),PYO3_PYTHON=str(PYTHON))
def guard():assert git('rev-parse','HEAD')==ENGINE and not git('status','--porcelain') and digest(interpreter)==ish
save()
try:
 cmd=['bazel','build','-c','opt','//bindings/python:_hawdb','//bindings/python:_hawdb_boundary_profile','--repo_env=CARGO_BAZEL_ISOLATED=false','--repo_env=CARGO_NET_OFFLINE=true'];row={'name':'native-build','command':cmd,'started':time.time(),'log':str(B/'986-final-held-write-controls-0c298984-build.log')};s['steps'].append(row);save()
 with Path(row['log']).open('w') as f:r=subprocess.run(cmd,cwd=W,env=env,stdout=f,stderr=subprocess.STDOUT)
 row.update(exit=r.returncode,ended=time.time());save();assert r.returncode==0;guard();packages={}
 for role,lib in [('ordinary','lib_hawdb.dylib'),('instrumented','lib_hawdb_boundary_profile.dylib')]:
  F=B/('final-held-write-'+role+'-0c298984-frozen');assert not F.exists();F.mkdir();shutil.copytree(W/'bindings/python/python/hawdb',F/'hawdb',ignore=shutil.ignore_patterns('__pycache__','*.pyc','*.so','*.dylib'));native=(W/'bazel-bin/bindings/python'/lib).resolve(strict=True);shutil.copyfile(native,F/'hawdb/_hawdb.so')
  for name in ['retained_node_write_control.py','retained_write_control.py']:shutil.copyfile(W/'bindings/benchmarks'/name,F/name)
  files={str(p.relative_to(F)):digest(p) for p in F.rglob('*') if p.is_file()};packages[role]={'path':str(F),'files_sha256':files,'native_build_output':str(native)}
 s.update(status='running',packages=packages,interpreter={'path':str(interpreter),'sha256':ish});save()
 def frozen_guard(role):
  guard();F=Path(packages[role]['path']);assert all(digest(F/p)==h for p,h in packages[role]['files_sha256'].items());return F
 # Mirror four historical fixtures for both ordinary/profiled Node producers.
 for rows,pad in [(1000,4096),(10000,64)]:
  for backend in ['memory','file']:
   for role in ['ordinary','instrumented']:
    F=frozen_guard(role);name=f'node-{rows}-{pad}-{backend}-{role}';cmd=[str(PYTHON),'-B',str(F/'retained_node_write_control.py'),'--package',str(F),'--rows',str(rows),'--pad-bytes',str(pad),'--backend',backend,'--hold-rows','32','--samples','8'];record={'kind':'node','rows':rows,'pad_bytes':pad,'backend':backend,'producer':role,'command':cmd,'started':time.time(),'stdout':str(OUT/(name+'.json')),'stderr':str(OUT/(name+'.stderr'))};s['records'].append(record);save()
    with Path(record['stdout']).open('w') as out,Path(record['stderr']).open('w') as err:r=subprocess.run(cmd,cwd=F,env=env,stdout=out,stderr=err)
    record.update(exit=r.returncode,ended=time.time());save();frozen_guard(role);assert r.returncode==0,name
    report=json.loads(Path(record['stdout']).read_text());assert report['status']=='ok' and report['profiled']==(role=='instrumented');record['sha256']=digest(Path(record['stdout']));save()
 # Same final-source numeric held-cursor fixtures; historical uncached prototype stays historical.
 for rows,pad in [(1000,4096),(10000,64)]:
  for backend in ['memory','file']:
   role='ordinary';F=frozen_guard(role);name=f'numeric-{rows}-{pad}-{backend}-{role}';cmd=[str(PYTHON),'-B',str(F/'retained_write_control.py'),'--rows',str(rows),'--pad-bytes',str(pad),'--backend',backend,'--samples','7'];record={'kind':'numeric','rows':rows,'pad_bytes':pad,'backend':backend,'producer':role,'command':cmd,'started':time.time(),'stdout':str(OUT/(name+'.json')),'stderr':str(OUT/(name+'.stderr'))};s['records'].append(record);save()
   with Path(record['stdout']).open('w') as out,Path(record['stderr']).open('w') as err:r=subprocess.run(cmd,cwd=F,env=env,stdout=out,stderr=err)
   record.update(exit=r.returncode,ended=time.time());save();frozen_guard(role);assert r.returncode==0,name;report=json.loads(Path(record['stdout']).read_text());assert report['status']=='ok';record['sha256']=digest(Path(record['stdout']));save()
 s.update(status='all_twelve_children_collected',ended=time.time());save();print('Existing final-source Node8/numeric4 held-write controls collected; independent audit remains required')
except BaseException as e:s.update(status='failed',error=str(e),ended=time.time());save();raise

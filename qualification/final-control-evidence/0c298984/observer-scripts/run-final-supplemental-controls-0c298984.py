from pathlib import Path
import hashlib,json,os,shutil,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'qualify-supplemental-file-final-0c298984';STATE=B/'986-final-supplemental-0c298984-runtime-controller-state.json';assert not STATE.exists()
s={'status':'waiting_for_preparation','pid':os.getpid(),'started':time.time(),'steps':[],'scope':'Twelve supplemental final-source file controls, separate from original90/110. Original failures remain preserved; no approval or whole-memory claim.'}
def save():
 p=STATE.with_suffix('.tmp');p.write_text(json.dumps(s,indent=2)+'\n');p.replace(STATE)
def digest(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def step(name,script):
 row={'name':name,'command':['python3',str(B/script)],'started':time.time(),'log':str(B/f'986-final-supplemental-0c298984-runtime-{name}.log')};s['steps'].append(row);save()
 with Path(row['log']).open('w') as f:r=subprocess.run(row['command'],stdout=f,stderr=subprocess.STDOUT)
 row.update(exit=r.returncode,ended=time.time());save()
 if r.returncode:raise RuntimeError(name+' failed')
save()
try:
 while True:
  q=json.loads((B/'986-final-supplemental-0c298984-prepare-state.json').read_text())
  if q['status']=='prepared_and_native_built':break
  if q['status']=='failed':raise RuntimeError('Preparation failed: '+q.get('error',''))
  os.kill(q['pid'],0);time.sleep(5)
 s.update(status='freezing',engine=q['engine'],observer_source=q['observer_source'],native_source=q['native_source']);save()
 F=B/'phase-checkpoint-0c298984-frozen-package';assert not F.exists();F.mkdir()
 shutil.copytree(W/'bindings/python/python/hawdb',F/'hawdb',ignore=shutil.ignore_patterns('__pycache__','*.pyc','*.so','*.dylib'))
 native=q['native_extensions']['lib_hawdb_retained_phase_profile.dylib'];assert digest(Path(native['path']))==native['sha256'];shutil.copyfile(native['path'],F/'hawdb/_hawdb.so');shutil.copyfile(W/'qualification/phases/consume.py',F/'consume.py');shutil.copyfile(W/'bindings/benchmarks/retained_boundary.py',F/'retained_boundary.py')
 hashes={str(p.relative_to(F)):digest(p) for p in F.rglob('*') if p.is_file()};assert hashes['retained_boundary.py']=='a9275fa36bd57feddc463a53b60f386d37bebba4337b4fee87040ba35ae3a9d1'
 interpreter=(B/'resource-qualification/venv/bin/python').resolve(strict=True)
 source={'engine':q['engine'],'engine_tree':'122ad7e9bcbe6ccc38e49dd018bed00c4d02c3a3','qualification_base':q['native_source'],'qualification_tree':q['native_tree'],'observer_source':q['observer_source'],'observer_tree':q['observer_tree'],'file_sha256':hashes,'interpreter':{'path':str(interpreter),'sha256':digest(interpreter)},'setup_checkpoint_rows':65536,'scope':'Opt-in supplemental fixture maintenance. Native inputs unchanged between bdb native source and three-file Python observer patch; original defaults/campaigns remain unchanged.'}
 (B/'phase-checkpoint-0c298984-frozen-source.json').write_text(json.dumps(source,indent=2)+'\n');(B/'phase-checkpoint-0c298984-committed-source.json').write_text(json.dumps({'head':q['observer_source'],'tree':q['observer_tree'],'engine':q['engine']},indent=2)+'\n')
 s['status']='running';save()
 for name,script in [('resources','run-resource-checkpoint-0c298984.py'),('resources-audit','audit-resource-checkpoint-0c298984.py'),('phases','run-phase-checkpoint-0c298984.py'),('phases-audit','audit-phase-checkpoint-0c298984.py')]:
  step(name,script)
  if name.endswith('-audit'):
   role='resource' if name=='resources-audit' else 'phase';a=json.loads((B/f'{role}-checkpoint-0c298984-full-audit.json').read_text());assert a['complete'] and a['engine']==q['engine'],name
 s.update(status='all_twelve_controls_audited',ended=time.time());save();print('All six resource and six phase controls independently audited')
except BaseException as e:s.update(status='failed',error=str(e),ended=time.time());save();raise

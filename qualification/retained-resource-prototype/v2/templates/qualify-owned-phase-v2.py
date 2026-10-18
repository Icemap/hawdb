from pathlib import Path
import hashlib,itertools,json,os,shutil,subprocess,sys,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');W=B/'qualify-phase-owned-v2';F=B/'phase-owned-v2-frozen-package';OUT=B/'phase-owned-v2-callback-probes';S=B/'phase-owned-v2-qualification-state.json';assert not S.exists() and not F.exists() and not OUT.exists();source=json.loads((B/'phase-owned-v2-frozen-source.json').read_text());PY=B/'resource-qualification/venv/bin/python'
s={'status':'building_phase_only_extension','pid':os.getpid(),'started':time.time(),'source_head':source['head'],'steps':[],'records':[],'scope':'Private predecessor v2 collector qualification only. Original complete consumer, fresh mode processes, memory/file1000rows,31samples plus warmup. No final986/full matrix, ordinary latency or performance-ratio claim.'}
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def save():S.write_text(json.dumps(s,indent=2)+'\n')
def guard():
 assert all(sha(W/p)==h for p,h in source['source_sha256'].items())
 assert sha(W/'bindings/python/Cargo.lock')==source['python_lock_sha256']
def run(name,args,cwd=W,env=None):
 guard();p=B/f'phase-owned-v2-{name}.log';d={'name':name,'command':args,'log':str(p),'started':time.time()};s['steps'].append(d);save()
 with p.open('w') as f:r=subprocess.run(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT)
 d.update(exit=r.returncode,ended=time.time());save();assert r.returncode==0,(name,p);guard()
try:
 save();guard()
 assert 'Finished' in (B/'phase-owned-v2-native-clippy.log').read_text() and 'Finished' in (B/'phase-owned-v2-python-clippy.log').read_text()
 run('extension-build',['bazel','build','-c','opt','//bindings/python:_hawdb_retained_phase_profile','--repo_env=CARGO_BAZEL_ISOLATED=false','--repo_env=CARGO_NET_OFFLINE=true','--build_event_json_file='+str(B/'phase-owned-v2-extension-build.bep.json')])
 F.mkdir();shutil.copytree(W/'bindings/python/python/hawdb',F/'hawdb',ignore=shutil.ignore_patterns('__pycache__','*.pyc','*.so','*.dylib'))
 extension=W/'bazel-bin/bindings/python/lib_hawdb_retained_phase_profile.dylib';assert extension.exists();shutil.copyfile(extension.resolve(),F/'hawdb/_hawdb.so')
 shutil.copyfile(W/'qualification/phases/consume.py',F/'consume.py');shutil.copyfile(W/'bindings/benchmarks/retained_boundary.py',F/'retained_boundary.py')
 consumer_sha=sha(F/'retained_boundary.py');old=json.loads((B/'phase-prototype-import-control-receipt.json').read_text());assert consumer_sha==old['consumer_sha256']
 hashes={str(p.relative_to(F)):sha(p) for p in F.rglob('*') if p.is_file()};ext_sha=sha(F/'hawdb/_hawdb.so');env={k:v for k,v in os.environ.items() if k not in ['PYTHONHOME','PYTHONPATH']};env.update(PYTHONPATH=str(F),PYTHONNOUSERSITE='1',PYTHONDONTWRITEBYTECODE='1')
 code='import hawdb._hawdb as n,pyarrow,sys,json; s=n._retained_phase_snapshot(); assert s["protocol"]=="hawdb-retained-phase-v2" and not s["allocation_instrumented"] and not s["overflowed"]; assert all(s[k]==0 for k in ["owned_calls","owned_ns","prepare_calls","prepare_ns","pull_calls","pull_ns"]); assert not hasattr(n,"_boundary_profile_snapshot"); assert pyarrow.__version__=="26.0.0"; print(json.dumps(dict(snapshot=s,python=sys.version,pyarrow=pyarrow.__version__,module=n.__file__)))'
 control=json.loads(subprocess.check_output([str(PY),'-c',code],cwd=F,env=env,text=True));assert Path(control['module']).resolve()==(F/'hawdb/_hawdb.so').resolve()
 control.update(extension_sha256=ext_sha,consumer_sha256=consumer_sha,package_sha256=hashes,scope='Import control opens no database; phase-only native profiler has allzero initial counters and allocator instrumentation absent.');(B/'phase-owned-v2-import-control.json').write_text(json.dumps(control,indent=2)+'\n');s.update(status='probing_complete_original_consumers',extension_sha256=ext_sha,consumer_sha256=consumer_sha);save();OUT.mkdir()
 for backend,mode in itertools.product(['memory','file'],['owned','retained','arrow']):
  guard();assert all(sha(F/p)==h for p,h in hashes.items());name=backend+'-'+mode;out=OUT/(name+'.json');err=OUT/(name+'.stderr');progress=OUT/(name+'-progress.json')
  cmd=[str(PY),str(F/'consume.py'),'--package',str(F),'--consumer',str(F/'retained_boundary.py'),'--extension',str(F/'hawdb/_hawdb.so'),'--consumer-sha256',consumer_sha,'--extension-sha256',ext_sha,'--rows','1000','--samples','31','--backend',backend,'--mode',mode,'--configuration','default','--progress-report',str(progress)]
  d={'backend':backend,'mode':mode,'command':cmd,'output':str(out),'stderr':str(err),'started':time.time()};s['records'].append(d);save()
  with out.open('w') as f,err.open('w') as e:r=subprocess.run(cmd,cwd=F,env=env,stdout=f,stderr=e)
  d.update(exit=r.returncode,ended=time.time());save();assert r.returncode==0,(name,err)
  report=json.loads(out.read_text());assert report['terminal'] and report['all_paths_succeeded'] and len(report['records'])==31
  for row in [report['warmup'],*report['records']]:
   assert row['status']=='ok' and row['parity'] and row['rows']==1000 and row['phase_control_valid'] and row['phase_attribution_valid'] and row['native_work_ns']>0 and row['handoff_boundary_residual_ns']>=0
   phases=row['native_phases']
   if mode=='owned':assert phases['owned_calls']==1 and phases['owned_ns']>0 and all(phases[k]==0 for k in ['prepare_calls','prepare_ns','pull_calls','pull_ns'])
   else:assert phases['prepare_calls']==1 and phases['pull_calls']>0 and phases['owned_calls']==phases['owned_ns']==0
  d.update(report_sha256=sha(out),measured_samples=31,all_phase_attribution_valid=True);save()
 guard();assert all(sha(F/p)==h for p,h in hashes.items());s.update(status='passed_all_three_mode_native_coverage',measured_samples=186,owned_measured_samples=62,warmups=6,ended=time.time());save()
except BaseException as e:s.update(status='failed',error=repr(e),ended=time.time());save();raise

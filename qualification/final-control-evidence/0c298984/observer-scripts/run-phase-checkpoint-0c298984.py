from pathlib import Path
import hashlib,json,os,subprocess,time
B=Path('/private/tmp/hawdb-wip-ready-20261015');F=B/'phase-checkpoint-0c298984-frozen-package';OUT=B/'phase-checkpoint-0c298984-results';OUT.mkdir(exist_ok=False)
source=json.loads((B/'phase-checkpoint-0c298984-frozen-source.json').read_text());S=B/'phase-checkpoint-0c298984-runtime-state.json'
s=dict(status='running',pid=os.getpid(),started=time.time(),source=source,records=[],scope='Supplemental explicit-checkpoint fixture; not original matrix replacement or performance acceptance.')
def save():S.write_text(json.dumps(s,indent=2)+'\n')
def guard():
 assert hashlib.sha256(Path(source['interpreter']['path']).read_bytes()).hexdigest()==source['interpreter']['sha256']
 assert all(hashlib.sha256((F/p).read_bytes()).hexdigest()==h for p,h in source['file_sha256'].items())
try:
 save()
 for rows in [1000001,1000000]:
  for mode in ['owned','retained','arrow']:
   guard();name=str(rows)+'-'+mode
   cmd=[str(B/'resource-qualification/venv/bin/python'),'-B',str(F/'consume.py'),'--package',str(F),'--extension',str(F/'hawdb/_hawdb.so'),'--consumer',str(F/'retained_boundary.py'),'--consumer-sha256',source['file_sha256']['retained_boundary.py'],'--extension-sha256',source['file_sha256']['hawdb/_hawdb.so'],'--rows',str(rows),'--samples','31','--backend','file','--mode',mode,'--configuration','large','--setup-checkpoint-rows','65536','--progress-report',str(OUT/(name+'-progress.json'))]
   d=dict(rows=rows,mode=mode,command=cmd,started=time.time());s['records'].append(d);save()
   with (OUT/(name+'.json')).open('w') as out,(OUT/(name+'.stderr')).open('w') as err:r=subprocess.run(cmd,cwd=F,stdout=out,stderr=err,env={**{k:v for k,v in os.environ.items() if k not in ['PYTHONPATH','PYTHONHOME']},'PYTHONNOUSERSITE':'1','PYTHONDONTWRITEBYTECODE':'1'})
   d.update(exit=r.returncode,ended=time.time());save();guard()
   report=json.loads((OUT/(name+'.json')).read_text());d.update(setup_completed_rows=report.get('setup_completed_rows'),setup_checkpoint_count=len(report['setup_checkpoints']),samples=len(report['records']),setup_or_probe_error=report.get('setup_or_probe_error'),report_sha256=hashlib.sha256((OUT/(name+'.json')).read_bytes()).hexdigest());save()
 s.update(status='collected',ended=time.time());save()
except BaseException as e:s.update(status='collector_failed',error=repr(e),ended=time.time());save();raise

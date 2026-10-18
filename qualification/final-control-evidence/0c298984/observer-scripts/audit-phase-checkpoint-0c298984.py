from pathlib import Path
import hashlib,importlib.util,json,subprocess
B=Path('/private/tmp/hawdb-wip-ready-20261015');F=B/'phase-checkpoint-0c298984-frozen-package';W=B/'qualify-supplemental-file-final-0c298984'
s=json.loads((B/'phase-checkpoint-0c298984-frozen-source.json').read_text());commit=json.loads((B/'phase-checkpoint-0c298984-committed-source.json').read_text())
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=W,text=True).strip()==commit['head']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=W,text=True).strip()
for p,h in s['file_sha256'].items():assert hashlib.sha256((F/p).read_bytes()).hexdigest()==h
assert (F/'consume.py').read_bytes()==(W/'qualification/phases/consume.py').read_bytes()
spec=importlib.util.spec_from_file_location('original_phase_audit',B/'audit-final-phase-0c298984.py');audit=importlib.util.module_from_spec(spec);spec.loader.exec_module(audit)
results=[];missing=[]
for n in [1000001,1000000]:
 for mode in ['owned','retained','arrow']:
  p=B/'phase-checkpoint-0c298984-results'/(str(n)+'-'+mode+'.json')
  if not p.exists() or not p.read_text().strip():missing.append([n,mode]);continue
  r=json.loads(p.read_text());assert r['terminal'] and r['setup_checkpoint_rows']==65536 and r['setup_completed_rows']==n
  assert [x['completed_rows'] for x in r['setup_checkpoints']]==[65536*i for i in range(1,16)]
  assert all(type(x['elapsed_ns']) is int and x['elapsed_ns']>0 for x in r['setup_checkpoints'])
  v=audit.audit_mode(r,mode,('large',n,'file'),s['file_sha256']['hawdb/_hawdb.so'])
  assert v['complete'] and v['observer_valid'] and v['setup_or_probe_error'] is None
  assert v['row_boundary_proven'] if n==1000001 else v['query_success']
  results.append(dict(rows=n,mode=mode,setup_checkpoints=15,setup_ns=r['setup_ns'],report_sha256=hashlib.sha256(p.read_bytes()).hexdigest(),audit=v))
out=dict(engine=s['engine'],supplemental_source=commit['head'],groups=results,missing=missing,complete=len(results)==6,acceptance_approved=False,scope='Supplemental file explicit-checkpoint fixture only; same frozen native producer and full consumer. Original failure retained; no ordinary latency ratio or original matrix substitution.')
(B/'phase-checkpoint-0c298984-full-audit.json').write_text(json.dumps(out,indent=2)+'\n')
print(json.dumps({'observed':len(results),'expected':6,'row_boundary_proven':[x['mode'] for x in results if x['rows']==1000001],'positive':[x['mode'] for x in results if x['rows']==1000000],'missing':missing}))

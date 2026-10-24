#!/usr/bin/env bash
set -euo pipefail
python3 - <<'PROOF'
import json,pathlib,hashlib,subprocess,importlib.util
q=pathlib.Path('/evidence');b=json.loads((q/'binary.json').read_text());binary=pathlib.Path(b['path']);assert hashlib.sha256(binary.read_bytes()).hexdigest()==b['sha256']
spec=importlib.util.spec_from_file_location('measurement','/work/scripts/measure-consumer-publication.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
records=[]
for name,writers,documents,payload in [('one-writer',1,32,128),('eight-writers',8,512,1024)]:
 command=[str(binary),'--root','/fixtures/read-probe-proof-1-'+name,'--mode','detached','--writers',str(writers),'--documents',str(documents),'--payload',str(payload),'--writes','256','--quiet-samples','1','--readers',str(writers),'--reads-per-reader','16']
 r=subprocess.run(command,text=True,capture_output=True);(q/(name+'.stdout')).write_text(r.stdout);(q/(name+'.stderr')).write_text(r.stderr)
 records.append({'case':name,'command':command,'returncode':r.returncode});(q/'processes.json').write_text(json.dumps(records,indent=2)+'\n')
 if r.returncode:raise SystemExit(r.returncode)
 x=json.loads(r.stdout);assert x['foreground_read']['requests']==writers*16
 overlap=m.validate_reads(x,writers,16);records[-1]['read_overlap_qualified']=overlap
 (q/'processes.json').write_text(json.dumps(records,indent=2)+'\n')
 print(name,'exact checks passed; overlap=',overlap,flush=True)
PROOF

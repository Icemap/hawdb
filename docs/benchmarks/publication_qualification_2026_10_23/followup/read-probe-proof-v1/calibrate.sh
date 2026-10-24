#!/usr/bin/env bash
set -euo pipefail
python3 - <<'CALIBRATE'
from pathlib import Path
import json,subprocess,hashlib,importlib.util
q=Path('/evidence');b=json.loads((q/'binary.json').read_text());binary=Path(b['path']);assert hashlib.sha256(binary.read_bytes()).hexdigest()==b['sha256']
spec=importlib.util.spec_from_file_location('measurement','/work/scripts/measure-consumer-publication.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
records=[]
for documents,payload in [(32,128),(512,1024)]:
 for mode in ['disabled','scoped-blocking','detached']:
  name=f'calibration-{documents}-{mode}'
  command=[str(binary),'--root','/fixtures/read-probe-calibration-1-'+name,'--mode',mode,'--writers','1','--documents',str(documents),'--payload',str(payload),'--writes','4096','--quiet-samples','32','--readers','1','--reads-per-reader','512']
  r=subprocess.run(command,text=True,capture_output=True);(q/(name+'.stdout')).write_text(r.stdout);(q/(name+'.stderr')).write_text(r.stderr)
  records.append({'case':name,'command':command,'returncode':r.returncode});(q/'calibration-processes.json').write_text(json.dumps(records,indent=2)+'\n')
  if r.returncode:raise SystemExit(r.returncode)
  x=json.loads(r.stdout);records[-1]['read_overlap_qualified']=m.validate_reads(x,1,512);records[-1]['completed_during_preparation']=x['foreground_read']['completed_during_preparation'];records[-1]['completed_with_active_writers']=x['foreground_read']['completed_with_active_writers']
  (q/'calibration-processes.json').write_text(json.dumps(records,indent=2)+'\n');print(name,records[-1]['read_overlap_qualified'],flush=True)
CALIBRATE

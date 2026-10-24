#!/usr/bin/env bash
set -euo pipefail
python3 - <<'CALIBRATE'
from pathlib import Path
import json,subprocess,hashlib,importlib.util
q=Path('/evidence');binaries={name:json.loads((q/file).read_text()) for name,file in [('baseline','baseline-binary.json'),('gated','binary.json')]}
for b in binaries.values():assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256']
spec=importlib.util.spec_from_file_location('measurement','/work/scripts/measure-consumer-publication.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
protocol={'rounds':2,'modes':['disabled','scoped-blocking','detached'],'documents':32,'payload':128,'writers':1,'readers':1,'writes':4096,'reads_per_reader':512,'quiet_samples':32,'order':'baseline/gated alternating by pair; no host compiler/fuzz active; calibration only, no latency qualification'}
(q/'calibration-protocol.json').write_text(json.dumps(protocol,indent=2)+'\n');records=[]
for round_index in range(2):
 for mode_index,mode in enumerate(protocol['modes']):
  order=['baseline','gated'] if (round_index*3+mode_index)%2==0 else ['gated','baseline']
  for revision in order:
   name=f'calibration-{round_index}-{mode}-{revision}'
   command=[binaries[revision]['path'],'--root','/fixtures/read-probe-gated-calibration-1-'+name,'--mode',mode,'--writers','1','--documents','32','--payload','128','--writes','4096','--quiet-samples','32','--readers','1','--reads-per-reader','512']
   r=subprocess.run(command,text=True,capture_output=True);(q/(name+'.stdout')).write_text(r.stdout);(q/(name+'.stderr')).write_text(r.stderr)
   row={'case':name,'round':round_index,'mode':mode,'revision':revision,'command':command,'returncode':r.returncode};records.append(row);(q/'calibration-processes.json').write_text(json.dumps(records,indent=2)+'\n')
   if r.returncode:raise SystemExit(r.returncode)
   x=json.loads(r.stdout);row['read_overlap_qualified']=m.validate_reads(x,1,512);row['completed_during_preparation']=x['foreground_read']['completed_during_preparation'];row['completed_with_active_writers']=x['foreground_read']['completed_with_active_writers'];row['read_p95_ns']=x['foreground_read']['latency']['p95_ns'];row['first_read_owner_wait_ns']=x['foreground_read']['samples'][0]['owner_wait_plus_entry_ns']
   (q/'calibration-processes.json').write_text(json.dumps(records,indent=2)+'\n');print(name,row['read_overlap_qualified'],row['first_read_owner_wait_ns'],flush=True)
CALIBRATE

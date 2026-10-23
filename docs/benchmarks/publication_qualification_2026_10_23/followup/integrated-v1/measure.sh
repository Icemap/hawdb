#!/usr/bin/env bash
set -euo pipefail
python3 - <<'MEASURE'
from pathlib import Path
import hashlib,json,subprocess
q=Path('/evidence');binary=json.loads((q/'binary.json').read_text());assert hashlib.sha256(Path(binary['path']).read_bytes()).hexdigest()==binary['sha256']
source=Path('/work');protocol=json.loads((q/'protocol.json').read_text())
for name,expected in protocol['overlay_sha256'].items():assert hashlib.sha256((source/name).read_bytes()).hexdigest()==expected
processes=[]
for name,sustained,writes in [('sustained',True,100000),('controls',False,4096)]:
 command=['python3','/work/scripts/measure-consumer-publication.py','--binary',binary['path'],'--build-artifacts','/evidence/build.jsonl','--output',str(q/name),'--fixture-parent','/fixtures/integrated-1-'+name,'--writes',str(writes),'--rounds','3','--quiet-samples','32']
 if sustained:command+=['--sustained']
 with (q/(name+'.log')).open('x') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 processes.append({'cohort':name,'command':command,'returncode':r.returncode});(q/'processes.json').write_text(json.dumps(processes,indent=2)+'\n')
 receipt=q/name/'receipt.json'
 if not receipt.exists():raise SystemExit(name+' incomplete; original failure retained; no dependent run')
 d=json.loads(receipt.read_text());assert d['process_and_exact_checks_passed'] and d['cases_complete']==(18 if sustained else 54)
 if r.returncode and not (sustained and r.returncode==1 and not d['sustained_progress_qualified']):raise SystemExit(r.returncode)
 # An unmet completed progress cohort never passes; independent controls still run.
if any(p['returncode'] for p in processes):raise SystemExit('All declared cases retained, but sustained acceptance failed')
MEASURE

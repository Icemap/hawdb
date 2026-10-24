#!/usr/bin/env bash
set -euo pipefail
python3 - <<'MEASURE'
from pathlib import Path
import json,subprocess,hashlib
q=Path('/evidence');source=Path('/work');protocol=json.loads((q/'protocol.json').read_text());freeze=json.loads((q/'source-freeze.json').read_text());b=json.loads((q/'binary.json').read_text());assert hashlib.sha256(Path(b['path']).read_bytes()).hexdigest()==b['sha256']
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=source,text=True).strip()==protocol['source'];assert not subprocess.check_output(['git','status','--porcelain'],cwd=source).strip()
for name,expected in freeze['files'].items():assert hashlib.sha256((source/name).read_bytes()).hexdigest()==expected,name
records=[]
for cohort in protocol['cohorts']:
 name=cohort['name'];command=['python3','/work/scripts/measure-consumer-publication.py','--binary',b['path'],'--build-artifacts','/evidence/build.jsonl','--output',str(q/name),'--fixture-parent','/fixtures/vector-publication-8202d2f50cc0-'+name,'--writes',str(cohort['writes']),'--rounds','3','--quiet-samples','32']+cohort['extra']
 with (q/(name+'.log')).open('x') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
 records.append({'cohort':name,'command':command,'returncode':r.returncode});(q/'processes.json').write_text(json.dumps(records,indent=2)+'\n')
 receipt=q/name/'receipt.json'
 if not receipt.exists():raise SystemExit(name+' incomplete; failure retained; no dependent run')
 result=json.loads(receipt.read_text());assert result['process_and_exact_checks_passed'] and result['cases_complete']==cohort['cases']
 # Completed but unqualified cohorts remain failed; independent cohorts still run.
if any(row['returncode'] for row in records):raise SystemExit('Complete evidence retained; at least one declared qualification failed')
MEASURE

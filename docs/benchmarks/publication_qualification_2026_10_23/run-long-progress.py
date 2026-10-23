"""Separate predeclared fixed-work progress cohort; no latency acceptance claim."""
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json
import os
import subprocess
import time

q = Path('/evidence/linux')
binary = Path((q / 'binary-path.txt').read_text())
out = q / 'long-progress'
out.mkdir(exist_ok=False)
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd='/work', text=True).strip() == '0f641a98f17898a8ea3dae6ddaaa46d641604ae3'
assert not subprocess.check_output(['git', 'status', '--porcelain'], cwd='/work', text=True)
expected_hash = json.loads((q / 'binary.json').read_text())['sha256']
assert hashlib.sha256(binary.read_bytes()).hexdigest() == expected_hash
protocol = json.loads(Path('/evidence/long-progress-protocol.json').read_text())
protocol.update(started_utc=datetime.now(timezone.utc).isoformat(), binary_sha256=expected_hash,
                filesystem=subprocess.check_output(['df', '-T', '/fixtures'], text=True))
(out / 'protocol.json').write_text(json.dumps(protocol, indent=2) + '\n')
results = []
with (out / 'results.jsonl').open('x') as raw:
    for repeat in range(3):
        cases = [(docs, payload, writers) for docs, payload in [(32, 128), (512, 1024)] for writers in [1, 4, 8]]
        if repeat % 2:
            cases.reverse()
        for index, (docs, payload, writers) in enumerate(cases):
            fixture = Path('/fixtures') / f'long-progress-{os.getpid()}-{repeat}-{index}'
            command = [str(binary), '--mode', 'detached', '--writers', str(writers), '--documents', str(docs), '--payload', str(payload), '--writes', '100000', '--quiet-samples', '32', '--root', str(fixture)]
            started = time.monotonic()
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            if result.returncode:
                (out / 'failure.json').write_text(json.dumps({'command':command,'returncode':result.returncode,'stdout':result.stdout,'stderr':result.stderr},indent=2)+'\n')
                raise SystemExit('correctness/process failure; original evidence retained')
            row = json.loads(result.stdout)
            row['run'] = {'repeat':repeat,'command':command,'wall_seconds_including_setup':time.monotonic()-started}
            raw.write(json.dumps(row,separators=(',',':'))+'\n')
            raw.flush()
            results.append({'repeat':repeat,'documents':docs,'writers':writers,'simultaneous_progress_qualified':row['simultaneous_progress_qualified'],'simultaneous_committed_post_e_advances':row['simultaneous_committed_post_e_advances'],'verified_documents':row['verified_documents']})
            print(json.dumps(results[-1]),flush=True)
assert len(results) == 18
receipt = {'source':protocol['source'],'cases_complete':18,'expected_cases':18,'all_exact_checks_passed':True,'finite_progress_qualified':all(row['simultaneous_progress_qualified'] for row in results),'cases':results,'limits':'Finite fixed work; no open-loop steady state, starvation-freedom, or latency-improvement claim. This separate dataset does not replace the 54-case controls or historical adverse runs.'}
(out / 'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
if not receipt['finite_progress_qualified']:
    raise SystemExit('finite progress criterion not met in every declared case; complete evidence retained')

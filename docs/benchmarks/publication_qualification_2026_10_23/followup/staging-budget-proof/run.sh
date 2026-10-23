#!/usr/bin/env bash
set -euo pipefail
cp /work/Cargo.lock /diagnostic/Cargo.lock
cargo generate-lockfile --offline --manifest-path /diagnostic/Cargo.toml > /diagnostic/lock.log 2>&1
cargo bench --locked --manifest-path /diagnostic/Cargo.toml --bench staging_budget_proof --no-run --message-format=json --jobs 1 > /diagnostic/build.jsonl 2> /diagnostic/build.log
python3 - <<'BINARY'
import pathlib,json,hashlib,subprocess,time
q=pathlib.Path('/diagnostic'); rows=[json.loads(x) for x in (q/'build.jsonl').read_text().splitlines()]
paths=[x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x['target']['name']=='staging_budget_proof' and x.get('executable')]
assert len(paths)==1
p=pathlib.Path(paths[0]);(q/'binary.json').write_text(json.dumps({'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+'\n')
with (q/'results.jsonl').open('x') as output,(q/'run.stderr').open('x') as err:
 started=time.monotonic();r=subprocess.run([str(p)],stdout=output,stderr=err)
(q/'process.json').write_text(json.dumps({'returncode':r.returncode,'seconds_including_setup':time.monotonic()-started,'scope':'diagnostic, no qualified timing'},indent=2)+'\n')
raise SystemExit(r.returncode)
BINARY

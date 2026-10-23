#!/usr/bin/env bash
set -euo pipefail
cargo bench --locked --bench concurrent_writers --no-run --message-format=json --jobs 1 > /evidence/build-${LABEL}.jsonl 2> /evidence/build-${LABEL}.log
python3 - <<'PY'
import pathlib,json,hashlib,os,shutil
q=pathlib.Path('/evidence');label=os.environ['LABEL']
rows=[json.loads(x) for x in (q/f'build-{label}.jsonl').read_text().splitlines()]
paths=[x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x['target']['name']=='concurrent_writers' and x.get('executable')]
assert len(paths)==1
p=pathlib.Path(paths[0]);copy=q/f'{label}-diagnostic';shutil.copy2(p,copy)
(q/f'binary-{label}.json').write_text(json.dumps({'path':str(p),'sha256':hashlib.sha256(copy.read_bytes()).hexdigest()},indent=2)+'\n')
PY

#!/usr/bin/env bash
set -euo pipefail
cargo bench --locked --bench consumer_publication --no-run --message-format=json --jobs 1 > /evidence/build.jsonl 2> /evidence/build.log
python3 - <<'BINARY'
import pathlib,json,hashlib
q=pathlib.Path('/evidence');rows=[json.loads(x) for x in (q/'build.jsonl').read_text().splitlines()]
paths=[x['executable'] for x in rows if x.get('reason')=='compiler-artifact' and x['target']['name']=='consumer_publication' and x.get('executable')]
assert len(paths)==1
p=pathlib.Path(paths[0]);(q/'binary.json').write_text(json.dumps({'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+'\n')
BINARY

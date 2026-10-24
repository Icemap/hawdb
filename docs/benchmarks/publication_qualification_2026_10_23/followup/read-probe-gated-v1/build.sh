#!/usr/bin/env bash
set -euo pipefail
python3 - <<'BACKUP'
from pathlib import Path
import hashlib,shutil,json
src=Path('/build/release/deps/consumer_publication-ee5e1a723b428eea');assert hashlib.sha256(src.read_bytes()).hexdigest()=='25455e117490249729f64d813a3383cc17cc81605d7874b38676f58af799c4be'
dest=Path('/build/read-probe-baseline-v1/consumer');assert not dest.exists();dest.parent.mkdir(exist_ok=True);shutil.copyfile(src,dest);dest.chmod(0o755)
Path('/evidence/baseline-binary.json').write_text(json.dumps({'path':str(dest),'sha256':hashlib.sha256(dest.read_bytes()).hexdigest()},indent=2)+'\n')
BACKUP
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

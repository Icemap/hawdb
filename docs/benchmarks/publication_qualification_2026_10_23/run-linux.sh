#!/usr/bin/env bash
set -euo pipefail
cd /work
mkdir -p /evidence/linux
uname -a > /evidence/linux/uname.txt
rustc --version --verbose > /evidence/linux/rustc.txt
cargo --version > /evidence/linux/cargo.txt
git rev-parse HEAD > /evidence/linux/source-head.txt
git status --porcelain > /evidence/linux/source-status.txt
cargo bench --locked --bench consumer_publication --no-run --message-format=json --jobs 2 > /evidence/linux/build.jsonl 2> /evidence/linux/build.log
python3 - <<'PYTHON'
import json,pathlib,hashlib
q=pathlib.Path('/evidence/linux')
executables=[]
for line in (q/'build.jsonl').read_text().splitlines():
 x=json.loads(line)
 if x.get('reason')=='compiler-artifact' and x['target']['name']=='consumer_publication' and x.get('executable'):executables.append(x['executable'])
assert len(executables)==1,executables
p=pathlib.Path(executables[0]);(q/'binary.json').write_text(json.dumps({'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+'\n')
(q/'binary-path.txt').write_text(str(p))
PYTHON
binary_path=$(cat /evidence/linux/binary-path.txt)
python3 /work/scripts/measure-consumer-publication.py --binary "$binary_path" --output /evidence/linux/consumer-matrix --fixture-parent /fixtures --rounds 3 --writes 4096 --quiet-samples 32 > /evidence/linux/measurement.log 2>&1

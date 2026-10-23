#!/usr/bin/env bash
set -euo pipefail
cd /work
if ! command -v strace >/dev/null; then
    apt-get update > /evidence/writer-trace/apt-update.log 2>&1
    apt-get install --no-install-recommends -y strace time > /evidence/writer-trace/apt-install.log 2>&1
fi
cargo bench --locked --bench concurrent_writers --no-run --message-format=json --jobs 2 > /evidence/writer-trace/build.jsonl 2> /evidence/writer-trace/build.log
python3 - <<'PYTHON'
import pathlib,json,hashlib
q=pathlib.Path('/evidence/writer-trace');paths=[]
for line in (q/'build.jsonl').read_text().splitlines():
 x=json.loads(line)
 if x.get('reason')=='compiler-artifact' and x['target']['name']=='concurrent_writers' and x.get('executable'):paths.append(x['executable'])
assert len(paths)==1
p=pathlib.Path(paths[0]);(q/'binary.json').write_text(json.dumps({'path':str(p),'sha256':hashlib.sha256(p.read_bytes()).hexdigest()},indent=2)+'\n');(q/'binary-path.txt').write_text(str(p))
PYTHON
mkdir -p /fixtures/trace
binary_path=$(cat /evidence/writer-trace/binary-path.txt)
strace -f -yy -T -s 80 -e trace=fsync,fdatasync,openat,close,statx,rename,renameat,renameat2,unlink,unlinkat,link,linkat,write,pwrite64 -o /evidence/writer-trace/syscalls.log "$binary_path" --writers 1 --readers 0 --round 0 --fixture-parent /fixtures/trace --reopen-before-measure > /evidence/writer-trace/results.jsonl 2> /evidence/writer-trace/run.log

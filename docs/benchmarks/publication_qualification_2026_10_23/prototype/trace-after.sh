#!/usr/bin/env bash
set -euo pipefail
python3 - <<'VERIFY'
import hashlib,json,pathlib
p=pathlib.Path('/build/release/deps/concurrent_writers-ba954da0915e1c32')
expected='8d622c83d767002d42729b1fa934ccecec077140977c3d57cc781bee073c10ad'
actual=hashlib.sha256(p.read_bytes()).hexdigest()
assert actual==expected
pathlib.Path('/evidence/binary.json').write_text(json.dumps({'path':str(p),'sha256':actual},indent=2)+'\n')
VERIFY
mkdir /fixtures/trace-after-1
strace -f -yy -T -s 80 -e trace=fsync,fdatasync,openat,close,statx,rename,renameat,renameat2,unlink,unlinkat,link,linkat,write,pwrite64 -o /evidence/syscalls.log /build/release/deps/concurrent_writers-ba954da0915e1c32 --writers 1 --readers 0 --round 0 --fixture-parent /fixtures/trace-after-1 --reopen-before-measure > /evidence/results.jsonl 2> /evidence/run.log

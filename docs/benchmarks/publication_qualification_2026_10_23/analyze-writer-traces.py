from pathlib import Path
import collections,json,re
q=Path('/private/tmp/hawdb-publication-qualification')
summary={}
for label,path in [('before',q/'writer-trace/syscalls.log'),('after',q/'prototype/writer-trace-after/syscalls.log')]:
 counts=collections.defaultdict(collections.Counter)
 for line in path.read_text().splitlines():
  m=re.match(r'^(\d+)\s+([a-z0-9_]+)\(',line)
  if m:counts[int(m[1])][m[2]]+=1
 writers=[(tid,c) for tid,c in sorted(counts.items()) if c['fdatasync']==256]
 assert len(writers)==4,(label,writers)
 summary[label]=[{'case':i+2,'thread':tid,**{k:c[k] for k in ['statx','openat','close','write','fdatasync','fsync']}} for i,(tid,c) in enumerate(writers)]
p=q/'prototype/writer-trace-comparison.json';p.write_text(json.dumps(summary,indent=2)+'\n');print(p.read_text())

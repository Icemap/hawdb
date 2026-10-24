from pathlib import Path
import gzip,hashlib,json,shutil
q=Path('/private/tmp/hawdb-publication-qualification')
out=Path('/private/tmp/hawdb-optimizer-work/docs/benchmarks/publication_qualification_2026_10_23')
out.mkdir(parents=True,exist_ok=True)
index=[]
def copy_file(source,destination):
 data=source.read_bytes()
 destination.parent.mkdir(parents=True,exist_ok=True)
 stored=destination
 if len(data)>100000:
  stored=destination.with_name(destination.name+'.gz')
  stored.write_bytes(gzip.compress(data,mtime=0))
 else:stored.write_bytes(data)
 index.append({'path':str(stored.relative_to(out)),'sha256':hashlib.sha256(stored.read_bytes()).hexdigest(),'uncompressed_sha256':hashlib.sha256(data).hexdigest(),'uncompressed_bytes':len(data)})
for directory in ['writer-control-39962682772','writer-matrix-39962691064-retry-1','linux','writer-trace','preflight-failure-1']:
 for source in sorted((q/directory).rglob('*')):
  if source.is_file() and source.name not in ['baseline-concurrent-writers','candidate-concurrent-writers']:
   copy_file(source,out/directory/source.relative_to(q/directory))
for filename in ['protocol.json','container-identity.json','image.json','long-progress-protocol.json','run-linux.sh','run-long-progress.py','run-writer-trace.sh','writer-control-job.log','writer-matrix-job.log','initial-alternates.txt']:
 copy_file(q/filename,out/filename)
p=q/'prototype'
for filename in ['source.json','source.patch','checkpointed-progress-source.patch','checkpointed-progress-protocol.json','binaries.json','build.jsonl','build.log','feasibility.py','progress-failure-reviewed.json','default-controls-command.json','default-controls-process.json','default-controls-controller.log','writer-trace-comparison.json','trace-after.sh','trace-after-controller.py','trace-tool-image.txt']:
 if (p/filename).exists():copy_file(p/filename,out/'prototype'/filename)
for directory in ['feasibility','checkpointed-progress','default-controls','writer-trace-after']:
 for source in sorted((p/directory).rglob('*')):
  if source.is_file():copy_file(source,out/'prototype'/directory/source.relative_to(p/directory))
for directory in ['writer-control-39962682772','writer-matrix-39962691064-retry-1']:
 binaries={name:hashlib.sha256((q/directory/name).read_bytes()).hexdigest() for name in ['baseline-concurrent-writers','candidate-concurrent-writers']}
 destination=out/directory/'binary-sha256.json';destination.write_text(json.dumps(binaries,indent=2)+'\n')
(out/'raw-artifact-index.json').write_text(json.dumps(index,indent=2)+'\n')
print('packaged',len(index),'raw text artifacts; binaries and database fixtures excluded')

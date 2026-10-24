"""Correctness/interface proof, not latency acceptance or the full progress cohort."""
from pathlib import Path
import hashlib,json,subprocess,tempfile,time
q=Path('/evidence/prototype');out=q/'feasibility';out.mkdir(exist_ok=False)
binary=json.loads((q/'binaries.json').read_text())['consumer_publication']
assert hashlib.sha256(Path(binary['path']).read_bytes()).hexdigest()==binary['sha256']
for name,writes,sustained in [('default-smoke',64,False),('checkpointed-proof',20000,True)]:
 parent=Path(tempfile.mkdtemp(prefix=name+'-',dir='/fixtures'))
 command=[binary['path'],'--mode','detached','--writers','1','--documents','32','--payload','128','--writes',str(writes),'--quiet-samples','1','--root',str(parent/'database')]
 if sustained:command+=['--sustained','true']
 started=time.monotonic();r=subprocess.run(command,capture_output=True,text=True,check=False)
 (out/(name+'.stdout')).write_text(r.stdout);(out/(name+'.stderr')).write_text(r.stderr)
 (out/(name+'-process.json')).write_text(json.dumps({'command':command,'returncode':r.returncode,'seconds_including_setup':time.monotonic()-started,'scope':'correctness only, not qualified timing'},indent=2)+'\n')
 if r.returncode:raise SystemExit(name+' failed; original evidence retained')
 row=json.loads(r.stdout)
 assert row['verified_documents']==32 and row['writes']==writes
 assert row['source_epoch_after_concurrent_phase']==row['source_epoch_before']+writes+row['maintenance_trigger_writes']
 if sustained:assert [x[0] for x in row['sustained_workload']['checkpoint_samples_write_count_elapsed_ns']]==[16384]
 print(name,'PASS','progress',row['sustained_workload']['qualified'],flush=True)

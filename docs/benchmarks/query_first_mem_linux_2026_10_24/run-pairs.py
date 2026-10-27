import argparse, hashlib, itertools, json, os, pathlib, subprocess, time
root=pathlib.Path(__file__).parent
assert not (root/'MEASUREMENTS_BLOCKED_SOURCE_ALIAS.json').exists(), 'Resolve source-alias build evidence before measurement'
parser=argparse.ArgumentParser()
parser.add_argument('--pilot',action='store_true')
args=parser.parse_args()
# The caller must wait for compilation/fuzz to finish before measurement.
# Refuse missing binaries; never silently reduce the matrix.
for name in ['baseline-latency','candidate-isolated-latency','baseline-instrumented','candidate-isolated-instrumented']:
 assert (root/'bin'/name).is_file(), name
rounds=1 if args.pilot else 3
run_root=root/('pilot-'+str(time.time_ns()) if args.pilot else 'paired-'+str(time.time_ns()))
run_root.mkdir()
sequence=0
for read_kind,out_of_core,writers,pair,mode in itertools.product(['point','report','stream'],[False,True],[1,2],range(rounds),['latency','instrumented']):
 order=['baseline','candidate'] if pair%2==0 else ['candidate','baseline']
 for side in order:
  label=f'{sequence:03}-{read_kind}-{int(out_of_core)}-{writers}-{pair}-{mode}-{side}'
  sequence+=1
  job={'path':str(run_root/(label+'-db')),'writers':writers,'readers':2,'writer_iterations':10 if args.pilot else 1000,'reader_iterations':20 if args.pilot else 4000,'rows':32 if args.pilot else 256,'read_kind':read_kind,'out_of_core':out_of_core}
  job_path=run_root/(label+'-job.json');job_path.write_text(json.dumps(job,indent=2)+'\n')
  binary=root/'bin'/((side if side=='baseline' else 'candidate-isolated')+'-'+mode)
  receipt={'label':label,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'job':job,'load_before':os.getloadavg(),'started_unix_ns':time.time_ns()}
  start=time.monotonic()
  with (run_root/(label+'-raw.json')).open('w') as output,(run_root/(label+'-stderr.log')).open('w') as error:
   status=subprocess.run([str(binary),str(job_path)],stdout=output,stderr=error)
  receipt.update(exit_code=status.returncode,elapsed_seconds=time.monotonic()-start,load_after=os.getloadavg())
  (run_root/(label+'-receipt.json')).write_text(json.dumps(receipt,indent=2)+'\n')
  print(json.dumps(receipt),flush=True)
  if status.returncode:
   print('FAILED; artifacts retained at '+str(run_root),flush=True)
   raise SystemExit(status.returncode)
print('COMPLETE '+str(run_root),flush=True)

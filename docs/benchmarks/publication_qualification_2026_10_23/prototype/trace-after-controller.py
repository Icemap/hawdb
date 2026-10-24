from pathlib import Path
import subprocess,json
q=Path('/private/tmp/hawdb-publication-qualification/prototype')
state=subprocess.check_output(['docker','wait','hawdb-1119-prototype-controls-1'],text=True).strip()
if state!='0':raise SystemExit('Controls failed; review original failure before tracing: '+state)
image=subprocess.check_output(['docker','commit','hawdb-1119-writer-trace-0f641a98f','hawdb-1119-trace-tools:local'],text=True).strip()
(q/'trace-tool-image.txt').write_text(image+'\n')
out=q/'writer-trace-after';out.mkdir(exist_ok=False)
command=['docker','run','--name','hawdb-1119-prototype-trace-1','--cpus','4','--memory','6g','--memory-swap','6g','--cap-add','SYS_PTRACE','--workdir','/work','--mount','type=bind,src=/private/tmp/hawdb-publication-qualification/source,dst=/work,readonly','--mount',f'type=bind,src={out},dst=/evidence','--mount',f'type=bind,src={q / "trace-after.sh"},dst=/trace-after.sh,readonly','--mount','type=volume,src=hawdb-1119-publication-build,dst=/build,readonly','--mount','type=volume,src=hawdb-1119-writer-trace-fixtures,dst=/fixtures',image,'bash','/trace-after.sh']
(out/'command.json').write_text(json.dumps(command,indent=2)+'\n')
with (out/'controller.log').open('x') as log:r=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT)
(out/'process.json').write_text(json.dumps({'returncode':r.returncode,'scope':'syscall work attribution only; no traced latency claim'},indent=2)+'\n')
raise SystemExit(r.returncode)

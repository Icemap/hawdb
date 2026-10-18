"""Attach perf only after the public-API query fixture finishes its warmups."""

import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import select
import shutil
import signal
import subprocess
import time

from audit_records import sha256


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('binary', type=Path)
parser.add_argument('fixture', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=False)
binary = args.output / 'rabitq-query-profile'
shutil.copy2(args.binary, binary)
package = Path('/tmp/hawdb-994-baseline/perf-package')
perf = package / 'usr/bin/perf'
environment = os.environ.copy()
environment['LD_LIBRARY_PATH'] = str(package / 'usr/lib')
control_read, control_write = os.pipe()
ack_read, ack_write = os.pipe()
target = recorder = None
report = {
    'state': 'starting', 'started_at': datetime.now(timezone.utc).isoformat(),
    'binary_sha256': sha256(binary), 'fixture': str(args.fixture),
    'perf_version': subprocess.check_output([str(perf), 'version'], env=environment, text=True).strip(),
    'perf_sha256': sha256(perf), 'load_before': os.getloadavg(),
    'scope': 'User CPU samples after three warmups, covering 100 full serving queries plus result validation and loop overhead. Not whole-application wall-time attribution.',
}
report_path = args.output / 'profile.json'
try:
    with (args.output / 'query-stderr.log').open('x') as target_errors, (args.output / 'perf.log').open('x') as perf_errors:
        target = subprocess.Popen([str(binary), 'profile', str(args.fixture)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=target_errors, text=True, bufsize=1)
        ready_deadline = time.monotonic() + 120
        while True:
            if target.poll() is not None:
                raise RuntimeError(f'query exited before readiness: {target.returncode}')
            if time.monotonic() >= ready_deadline:
                raise TimeoutError('query did not report readiness')
            if select.select([target.stdout], [], [], 1)[0]:
                ready = json.loads(target.stdout.readline())
                assert ready['phase'] == 'ready' and ready['pid'] == target.pid
                break
        command = [str(perf), 'record', '-e', 'cpu-clock:u', '-F', '997', '--strict-freq', '--call-graph', 'dwarf,16384', '--delay=-1', '--control', f'fd:{control_read},{ack_write}', '-p', str(target.pid), '-o', str(args.output / 'perf.data')]
        report.update({'state': 'profiling', 'query_ready': ready, 'command': command})
        report_path.write_text(json.dumps(report, indent=2) + '\n')
        recorder = subprocess.Popen(command, pass_fds=(control_read, ack_write), env=environment, stdout=perf_errors, stderr=subprocess.STDOUT)
        os.close(control_read)
        os.close(ack_write)
        control_read = ack_write = None
        os.write(control_write, b'enable\n')
        if not select.select([ack_read], [], [], 20)[0]:
            raise RuntimeError('perf did not acknowledge enabling samples')
        acknowledgement = os.read(ack_read, 16)
        report['perf_enable_ack_hex'] = acknowledgement.hex()
        if acknowledgement.rstrip(b'\0') != b'ack\n':
            raise RuntimeError('perf did not acknowledge enabling samples')
        target.stdin.write('run\n')
        target.stdin.flush()
        output, _ = target.communicate(timeout=180)
        completed = json.loads(output)
        assert target.returncode == 0 and completed['phase'] == 'complete'
        assert len(completed['elapsed_ns']) == ready['repetitions'] == 100
        try:
            perf_status = recorder.wait(timeout=10)
        except subprocess.TimeoutExpired:
            recorder.send_signal(signal.SIGINT)
            perf_status = recorder.wait(timeout=20)
        assert perf_status == 0, perf_status
        report.update({'state': 'recorded', 'query_completed': completed, 'perf_exit_code': perf_status, 'target_exit_code': target.returncode})
    for name, extra in [
        ('self-symbols.txt', ['report', '--stdio', '--no-children', '--inline', '--sort', 'symbol', '--percent-limit', '0.1']),
        ('callgraph.txt', ['report', '--stdio', '--children', '--inline', '--call-graph', 'graph,0.5,caller']),
    ]:
        with (args.output / name).open('x') as output:
            subprocess.run([str(perf), *extra, '-i', str(args.output / 'perf.data')], env=environment, stdout=output, stderr=subprocess.STDOUT, check=True)
    report['perf_data_sha256'] = sha256(args.output / 'perf.data')
    report['state'] = 'passed'
except BaseException as error:
    report.update({'state': 'failed', 'error': repr(error)})
    raise
finally:
    for process in [target, recorder]:
        if process is not None and process.poll() is None:
            process.terminate()
            process.wait(timeout=20)
    for descriptor in [control_read, control_write, ack_read, ack_write]:
        if descriptor is not None:
            os.close(descriptor)
    report['finished_at'] = datetime.now(timezone.utc).isoformat()
    report['load_after'] = os.getloadavg()
    report_path.write_text(json.dumps(report, indent=2) + '\n')
print(f'Profile recorded at {args.output}')

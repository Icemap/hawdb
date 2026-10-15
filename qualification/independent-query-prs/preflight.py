"""Fail before ordinary sampling if a frozen language entrypoint cannot run."""

from pathlib import Path
import json
import sys

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "harness/bindings/benchmarks"))
import compare
import run as matrix

matrix.ROOT = ROOT / "harness"
producers = json.loads((ROOT / "producers.json").read_text())
output = ROOT / "preflight"
output.mkdir(exist_ok=False)
fixture = matrix.fixture(1, "point")
records = []
for name, producer in producers.items():
    artifacts = producer["artifacts"]
    compare.require_producer(artifacts)
    for layer in ["rust", "python", "go"]:
        directory = output / (name + "-" + layer)
        directory.mkdir()
        job = directory / "job.json"
        job.write_text(
            json.dumps({**fixture, "backend": "memory", "path": str(directory / "db")})
        )
        command = [artifacts[layer]["path"], str(job)]
        if layer == "python":
            command.append(artifacts["python_extension"]["path"])
        if layer == "go":
            command.append(artifacts["library"]["path"])
        result = matrix.run_child(command, directory / "result")
        result.update(producer=name, layer=layer, diagnostic_only=True)
        records.append(result)
        (output / "report.json").write_text(json.dumps(records, indent=2) + "\n")
        assert result["process_exit"] == 0 and result["status"] == "ok", result
        assert (
            result["output_rows"] == 1
            and result["checksum"] == fixture["expected_checksum"]
        )
        print(name, layer, "preflight passes", flush=True)

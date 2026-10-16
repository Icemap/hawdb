"""Untimed typed-config opening adapter; timed consumer source is unchanged."""
import atexit
import importlib
import json
from pathlib import Path
import runpy
import sys
import hawdb
native = importlib.import_module("hawdb._hawdb")
observations = []
def configured_open(path=None):
    db = native._open_large_result_qualification(path)
    values = tuple(db._qualification_limits())
    assert values == (1000000, 1073741824, 1073741824, 1073741824, 4294967296, 268435456, 67108864), values
    observations.append(values)
    return db
hawdb.open = configured_open
@atexit.register
def record(output=Path(__file__).parent.parent / "observed-limits.json", dumps=json.dumps, observed=observations):
    output.write_text(dumps(observed) + "\n")
sys.argv = sys.argv[1:]
runpy.run_path(sys.argv[0], run_name="__main__")

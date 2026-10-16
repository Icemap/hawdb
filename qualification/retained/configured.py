"""Untimed typed-config opening adapter; timed consumer source is unchanged."""
import atexit
import importlib
import json
import os
from pathlib import Path
import runpy
import sys
import hawdb
native = importlib.import_module("hawdb._hawdb")
observations = []
original_open = hawdb.open
profile = os.environ.get("QUAL_CONFIGURATION", "large")
assert profile in ("default", "large"), profile
def configured_open(path=None):
    db = native._open_large_result_qualification(path) if profile == "large" else original_open(path)
    values = tuple(db._qualification_limits())
    if profile == "large":
        assert values == (1000000, 268435456, 1073741824, 268435456, 2147483648), values
    else:
        assert values[:4] == (100000, 67108864, 268435456, 16777216), values
    observations.append(values)
    return db
hawdb.open = configured_open
@atexit.register
def record(output=Path(__file__).parent.parent / "observed-limits.json", dumps=json.dumps, observed=observations):
    output.write_text(dumps(observed) + "\n")
sys.argv = sys.argv[1:]
runpy.run_path(sys.argv[0], run_name="__main__")

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
assert profile in ("default", "large")
def configured_open(path=None):
    db = native._open_large_result_qualification(path) if profile == "large" else original_open(path)
    values = tuple(db._qualification_limits())
    observations.append(values)
    if profile == "large":
        assert values == (1000000, 1073741824, 1073741824, 1073741824, 2147483648, 268435456, 67108864), values
    else:
        assert values[:4] == (0, 67108864, 268435456, 16777216), values
        assert values[5:] == (16777216, 67108864), values
    return db
hawdb.open = configured_open
@atexit.register
def record(output=Path(__file__).parent.parent / "observed-limits.json", dumps=json.dumps, observed=observations):
    output.write_text(dumps(observed) + "\n")
sys.argv = sys.argv[1:]
runpy.run_path(sys.argv[0], run_name="__main__")

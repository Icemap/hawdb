"""Untimed typed-config opening adapter; timed consumer source is unchanged."""
import importlib
import json
from pathlib import Path
import runpy
import sys
import hawdb
native = importlib.import_module("hawdb._hawdb")
observations = []
receipt = Path(__file__).resolve().parent.parent / "observed-limits.json"
def configured_open(path=None):
    db = native._open_large_result_qualification(path)
    values = tuple(db._qualification_limits())
    assert values == (1000000, 268435456, 1073741824, 67108864, 268435456, 2147483648), values
    observations.append(values)
    return db
hawdb.open = configured_open
sys.argv = sys.argv[1:]
try:
    runpy.run_path(sys.argv[0], run_name="__main__")
finally:
    receipt.write_text(json.dumps(observations) + "\n")

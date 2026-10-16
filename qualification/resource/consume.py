"""Apply explicit limits and optional complete-call observers outside consumers."""

import argparse
import importlib
import importlib.util
import json
from pathlib import Path
import sys

from observe import ResourceObserver, wrap_consume


LIMITS = (1000000, 1073741824, 1073741824, 1073741824, 2147483648, 268435456, 67108864)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--resources", type=Path)
    parser.add_argument("consumer", type=Path)
    parser.add_argument("arguments", nargs=argparse.REMAINDER)
    args = parser.parse_args(argv)
    import hawdb

    native = importlib.import_module("hawdb._hawdb")
    snapshot = getattr(native, "_boundary_profile_snapshot", None)
    assert (snapshot is not None) == (args.resources is not None)
    observer = ResourceObserver(args.resources, snapshot) if args.resources else None
    observations = []
    original_open = hawdb.open
    original_arguments = sys.argv

    def configured_open(path=None):
        database = native._open_large_result_qualification(path)
        values = tuple(database._qualification_limits())
        observations.append(values)
        if values != LIMITS:
            database.close()
            raise RuntimeError(f"explicit limit readback differs: {values}")
        return database

    spec = importlib.util.spec_from_file_location("qualified_numeric", args.consumer)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    if observer:
        wrap_consume(module, observer)
    hawdb.open = configured_open
    sys.argv = [str(args.consumer), *args.arguments]
    try:
        module.main()
    finally:
        hawdb.open = original_open
        sys.argv = original_arguments
        args.receipt.write_text(
            json.dumps(
                {
                    "limits": observations,
                    "expected_limits": LIMITS,
                    "instrumented": observer is not None,
                    "observer_errors": (
                        [
                            {"type": type(error).__name__, "message": str(error)}
                            for error in observer.observer_errors
                        ]
                        if observer
                        else []
                    ),
                }
            )
            + "\n"
        )


if __name__ == "__main__":
    main()

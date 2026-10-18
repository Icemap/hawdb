"""Prepare private phase qualification sources; never commit or run tests.

Only invoke on a separate qualification worktree after current validation.
The original timed consumer is hashed before/after and remains unchanged.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

FEATURE = "retained-phase-profiling"
HERE = Path(__file__).resolve().parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def instrument(text, function, phase):
    expression = re.compile(
        r"(    pub fn " + re.escape(function) + r"\([\s\S]*?\n    \) -> [^\n]+\{\n)"
    )
    guard = (
        f'        #[cfg(feature = "{FEATURE}")]\n'
        "        let _retained_phase = crate::retained_phase_profile::Timer::start(\n"
        f"            crate::retained_phase_profile::Phase::{phase},\n"
        "        );\n"
    )
    if phase == "Pull":
        guard = (
            f'        #[cfg(feature = "{FEATURE}")]\n'
            "        let _retained_phase =\n"
            "            crate::retained_phase_profile::Timer::start(crate::retained_phase_profile::Phase::Pull);\n"
        )
    result, count = expression.subn(lambda match: match[1] + guard, text)
    if count != 1:
        raise ValueError(f"expected exactly one {function}, found {count}")
    return result


def prepare(worktree, receipt, apply=False):
    consumer = worktree / "bindings/benchmarks/retained_boundary.py"
    consumer_hash = digest(consumer)
    changed = {}

    def update(relative, transform):
        path = worktree / relative
        original = path.read_text()
        updated = transform(original)
        if updated == original:
            raise ValueError(f"no edit: {relative}")
        changed[relative] = {"before": digest(path)}
        # Plan all edits first; an anchor mismatch must not partially apply.
        edits[path] = updated

    edits = {}
    update("Cargo.toml", lambda s: s.replace("[features]\n", f"[features]\n{FEATURE} = []\n", 1))
    update("src/lib.rs", lambda s: s.replace(
        "pub mod executor;\n",
        'pub mod executor;\n#[doc(hidden)]\n#[cfg(feature = "retained-phase-profiling")]\n'
        "pub mod retained_phase_profile;\n", 1))
    update("src/api/retained_query.rs", lambda s: instrument(
        instrument(instrument(s, "query_with_params_retained", "Prepare"),
                   "into_retained_query", "Prepare"), "next_batch_with_metadata", "Pull"))
    update("src/embedded.rs", lambda s: instrument(instrument(instrument(
        s, "query_with_params_retained", "Prepare"),
        "query_with_params_admitted", "Owned"), "query_with_params_admitted_context", "Owned"))
    update("bindings/python/Cargo.toml", lambda s: s.replace(
        "[features]\n", f'[features]\n{FEATURE} = ["hawdb/{FEATURE}"]\n', 1))
    hook = (HERE / "python_hook.rs").read_text()
    registration = (
        '    #[cfg(feature = "retained-phase-profiling")]\n'
        "    module.add_function(wrap_pyfunction!(_retained_phase_snapshot, module)?)?;\n"
    )
    update("bindings/python/src/lib.rs", lambda s: s.replace(
        "/// Report the engine capabilities", hook + "\n/// Report the engine capabilities", 1
    ).replace('    module.add("__version__", env!("CARGO_PKG_VERSION"))?;\n',
              '    module.add("__version__", env!("CARGO_PKG_VERSION"))?;\n' + registration, 1))

    def root_target(s):
        anchor = '\nrust_library(\n    name = "hawdb",'
        start = s.index(anchor) + 1
        end = s.index("\n)\n", start) + 3
        target = s[start:end]
        target = target.replace('name = "hawdb",', 'name = "hawdb_retained_phase_profile",', 1)
        target = target.replace("crate_features = _HAWDB_DEFAULT_FEATURES,",
                                'crate_features = _HAWDB_DEFAULT_FEATURES + ["retained-phase-profiling"],', 1)
        target = target.replace('    name = "hawdb_retained_phase_profile",',
                                '    name = "hawdb_retained_phase_profile",\n    tags = ["manual"],', 1)
        assert 'tags = ["manual"]' in target
        assert '"hawdb_retained_phase_profile"' not in s
        return s + "\n# Private qualification build; never a default production target.\n" + target

    def python_target(s):
        start = s.index("rust_shared_library(\n")
        end = s.index("\n)\n", start) + 3
        target = s[start:end]
        target = target.replace('name = "_hawdb",', 'name = "_hawdb_retained_phase_profile",', 1)
        target = target.replace('crate_name = "_hawdb",', 'crate_name = "_hawdb_retained_phase_profile",', 1)
        target = target.replace('crate_features = ["abi3"],',
                                'crate_features = ["abi3", "retained-phase-profiling"],', 1)
        target = target.replace('"//:hawdb",', '"//:hawdb_retained_phase_profile",', 1)
        target = target.replace('    version = "0.9.0",',
                                '    tags = ["manual"],\n    version = "0.9.0",', 1)
        assert '"_hawdb_retained_phase_profile"' not in s
        return s + "\n# Phase-only qualification: allocation/tracemalloc instrumentation is absent.\n" + target

    update("BUILD.bazel", root_target)
    update("bindings/python/BUILD.bazel", python_target)
    module = worktree / "src/retained_phase_profile.rs"
    assert not module.exists()
    edits[module] = (HERE / "retained_phase_profile.rs").read_text()
    changed[str(module.relative_to(worktree))] = {"before": None}

    def numeric_driver(s):
        def replace(old, new):
            nonlocal s
            if s.count(old) != 1:
                raise ValueError("resource driver anchor mismatch: " + old[:100])
            s = s.replace(old, new, 1)

        replace('    args = parser.parse_args()\n',
                '    parser.add_argument("--phase-only", action="store_true")\n'
                '    args = parser.parse_args()\n'
                '    assert not (args.phase_only and args.instrumented)\n')
        replace('    target = "//bindings/python:_hawdb_boundary_profile" if args.instrumented else "//bindings/python:_hawdb"\n',
                '    target = ("//bindings/python:_hawdb_retained_phase_profile" if args.phase_only\n'
                '              else "//bindings/python:_hawdb_boundary_profile" if args.instrumented\n'
                '              else "//bindings/python:_hawdb")\n')
        replace('    extension = ROOT / ("bazel-bin/bindings/python/lib" + ("_hawdb_boundary_profile" if args.instrumented else "_hawdb") + suffix)\n',
                '    extension_name = ("_hawdb_retained_phase_profile" if args.phase_only\n'
                '                      else "_hawdb_boundary_profile" if args.instrumented else "_hawdb")\n'
                '    extension = ROOT / ("bazel-bin/bindings/python/lib" + extension_name + suffix)\n')
        replace('    for relative in ("bindings/benchmarks/retained_boundary.py", "qualification/resources/numeric.py"):\n',
                '    observer = "qualification/phases/consume.py" if args.phase_only else "qualification/resources/numeric.py"\n'
                '    for relative in ("bindings/benchmarks/retained_boundary.py", observer):\n')
        replace('    (output / "manifest.json").write_text',
                '    manifest["phase_instrumented"] = args.phase_only\n'
                '    manifest["driver_sha256"] = digest(Path(__file__))\n'
                '    if args.phase_only:\n'
                '        manifest["scope"] = "Phase-only owned-native or retained prepare/pull and boundary residual. Separate from ordinary and allocation-instrumented measurements; no pure-conversion or whole-memory bound."\n'
                '    (output / "manifest.json").write_text')
        replace('str(frozen / "numeric.py"), "--package"',
                'str(frozen / Path(observer).name), "--package"')
        replace('        if args.instrumented:\n',
                '        if args.phase_only:\n'
                '            command += ["--consumer-sha256", digest(frozen / "retained_boundary.py"),\n'
                '                        "--extension-sha256", digest(frozen / "_hawdb.so"),\n'
                '                        "--progress-report", str(output / (mode + "-progress.json"))]\n'
                '        if args.instrumented:\n')
        return s

    update("qualification/resources/numeric_driver.py", numeric_driver)
    observer = worktree / "qualification/phases/consume.py"
    assert not observer.exists()
    edits[observer] = (HERE / "consume.py").read_text()
    changed[str(observer.relative_to(worktree))] = {"before": None}
    # This is deliberately not invoked by any existing validation/publication runner.
    if apply:
        for path, content in edits.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
    for relative, entry in changed.items():
        entry["after"] = hashlib.sha256(edits[worktree / relative].encode()).hexdigest()
    assert digest(consumer) == consumer_hash
    receipt.write_text(json.dumps({
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=worktree, text=True).strip(),
        "consumer_sha256": consumer_hash, "changes": changed,
        "worktree_diff_sha256": hashlib.sha256(subprocess.check_output(
            ["git", "diff", "--binary"], cwd=worktree)).hexdigest(),
        "status": "prepared_uncompiled" if apply else "planned_uncompiled", "feature": FEATURE,
        "scope": "Qualification overlay only; no latency/correctness claim until compiled real-consumer coverage."
    }, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--worktree", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--apply", action="store_true")
    args = parser.parse_args()
    prepare(args.worktree.resolve(), args.receipt.resolve(), args.apply)

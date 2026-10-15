"""Compare unchanged dense-identity assertions with phase logging on two sources."""

from pathlib import Path
import hashlib
import json
import os
import shutil
import signal
import subprocess
import time

TEST = "api::tests::projected_graph_execution::review_regressions::projected_identity_ignores_large_content_and_rejects_large_ids_without_a_prefix"
FILE = "src/api/tests/projected_graph_execution/review_regressions.rs"
SOURCES = {
    "baseline": (None, None),
    "1036": (
        "dc73bed5e13c10351e83a3e3b495c9043b41a2a9",
        "ec79975524707f7ee9b4b06cbd7ce92ce6e2a7f9",
    ),
}
BASELINE = "f7c4eb48915b58f5c800491869f93bdb8c0c73da"


def instrument(source):
    marker = "fn projected_identity_ignores_large_content_and_rejects_large_ids_without_a_prefix() {"
    assert source.count(marker) == 1
    prefix, body = source.split(marker, 1)
    body = "\n    let diagnostic_started = std::time::Instant::now();" + body

    def insert(needle, text):
        nonlocal body
        assert body.count(needle) == 1, needle
        body = body.replace(needle, text, 1)

    insert(
        "        store.checkpoint(&catalog).unwrap();\n    }",
        '        store.checkpoint(&catalog).unwrap();\n    }\n    eprintln!("identity-diagnostic stage=fixture-sealed elapsed={:?}", diagnostic_started.elapsed());',
    )
    insert(
        "        db.checkpoint().unwrap();\n    }",
        '        db.checkpoint().unwrap();\n    }\n    eprintln!("identity-diagnostic stage=projection-sealed elapsed={:?}", diagnostic_started.elapsed());',
    )
    insert(
        "            let mut config = DatabaseConfig {",
        '            eprintln!("identity-diagnostic stage=case-start residency={residency:?} budget={budget} elapsed={:?}", diagnostic_started.elapsed());\n            let mut config = DatabaseConfig {',
    )
    insert(
        "            let mut snapshot = db.begin_read_transaction().unwrap();",
        '            let mut snapshot = db.begin_read_transaction().unwrap();\n            eprintln!("identity-diagnostic stage=reader-opened residency={residency:?} budget={budget} elapsed={:?}", diagnostic_started.elapsed());',
    )
    insert(
        "            assert_eq!(rows.len(), 128);",
        '            eprintln!("identity-diagnostic stage=page-rank-finished residency={residency:?} budget={budget} elapsed={:?}", diagnostic_started.elapsed());\n            assert_eq!(rows.len(), 128);',
    )
    insert(
        "            assert_eq!(snapshot.commit_epoch(), epoch);",
        '            eprintln!("identity-diagnostic stage=identity-checks-finished residency={residency:?} budget={budget} elapsed={:?}", diagnostic_started.elapsed());\n            assert_eq!(snapshot.commit_epoch(), epoch);',
    )
    insert(
        "    std::fs::remove_dir_all(path).unwrap();",
        '    std::fs::remove_dir_all(path).unwrap();\n    eprintln!("identity-diagnostic stage=done elapsed={:?}", diagnostic_started.elapsed());',
    )
    return prefix + marker + body


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    root = Path(os.environ["RUNNER_TEMP"]) / "identity-diagnostic"
    root.mkdir(exist_ok=False)
    (root / "host.json").write_text(
        json.dumps(
            {
                command: subprocess.check_output(arguments, text=True)
                for command, arguments in [
                    ("platform", ["uname", "-a"]),
                    ("cpu", ["lscpu"]),
                    ("memory", ["free", "-b"]),
                ]
            },
            indent=2,
        )
        + "\n"
    )
    source = root / "source"
    subprocess.run(
        [
            "git",
            "clone",
            "--branch",
            "main",
            "https://github.com/nowledge-co/hawdb.git",
            str(source),
        ],
        check=True,
    )
    subprocess.run(
        ["git", "fetch", "origin", "refs/pull/1036/head"], cwd=source, check=True
    )

    def git(*args):
        return subprocess.check_output(["git", *args], cwd=source, text=True).strip()

    records = {}
    original_fixture = None
    for name, (feature, expected_tree) in SOURCES.items():
        assert not git("status", "--porcelain")
        subprocess.run(
            ["git", "checkout", "--detach", BASELINE], cwd=source, check=True
        )
        tree = git("rev-parse", "HEAD^{tree}")
        production_tree = (
            git("merge-tree", "--write-tree", BASELINE, feature) if feature else tree
        )
        assert expected_tree is None or production_tree == expected_tree
        subprocess.run(
            ["git", "checkout", production_tree, "--", "."], cwd=source, check=True
        )
        assert git("write-tree") == production_tree
        path = source / FILE
        original = path.read_text()
        if original_fixture is None:
            original_fixture = original
        assert original == original_fixture, "fixture source differs"
        path.write_text(instrument(original))
        subprocess.run(["git", "add", "--", FILE], cwd=source, check=True)
        patch = root / (name + "-logging.patch")
        patch.write_bytes(
            subprocess.check_output(
                ["git", "diff", "--cached", "--binary", production_tree], cwd=source
            )
        )
        full_patch = root / (name + "-source-overlay.patch")
        full_patch.write_bytes(
            subprocess.check_output(
                ["git", "diff", "--cached", "--binary", BASELINE], cwd=source
            )
        )
        measured_tree = git("write-tree")
        with (root / (name + "-build.log")).open("w") as log:
            subprocess.run(
                ["bazel", "build", "//:hawdb_unit_test_binary"],
                cwd=source,
                stdout=log,
                stderr=subprocess.STDOUT,
                check=True,
            )
        assert git("write-tree") == measured_tree
        subprocess.run(["git", "diff", "--exit-code"], cwd=source, check=True)
        binary = root / (name + "-test-binary")
        original_binary = (source / "bazel-bin/hawdb_unit_test_binary").resolve(
            strict=True
        )
        digest = sha(original_binary)
        shutil.copy2(original_binary, binary)
        assert digest == sha(binary) == sha(original_binary)
        records[name] = {
            "head": BASELINE,
            "feature_head": feature,
            "base_tree": tree,
            "production_tree": production_tree,
            "measured_tree": measured_tree,
            "logging_patch_sha256": sha(patch),
            "source_overlay_sha256": sha(full_patch),
            "fixture_sha256": hashlib.sha256(original.encode()).hexdigest(),
            "binary": str(binary),
            "binary_sha256": digest,
            "build": "bazel build //:hawdb_unit_test_binary; default configuration and fastbuild",
        }
        subprocess.run(
            ["git", "apply", "-R", "--index", str(full_patch)], cwd=source, check=True
        )
        assert not git("status", "--porcelain")
    assert len({p["logging_patch_sha256"] for p in records.values()}) == 1
    report = {
        "terminal": False,
        "producers": records,
        "test": TEST,
        "records": [],
        "scope": "One exact original test per source, same Linux host after both default-fastbuild builds finish. Only phase eprintln logging is added; all data, two residencies, two budgets, calls and assertions are unchanged. The 900-second owner deadline is retained. This is diagnostic evidence, not CI root acceptance or a qualified latency speedup.",
    }

    def save():
        (root / "report.json").write_text(json.dumps(report, indent=2) + "\n")

    save()
    for name, p in records.items():
        assert sha(Path(p["binary"])) == p["binary_sha256"]
        started = time.monotonic()
        with (root / (name + ".stdout")).open("w") as stdout, (
            root / (name + ".stderr")
        ).open("w") as stderr:
            process = subprocess.Popen(
                ["/usr/bin/time", "-v", p["binary"], "--exact", TEST, "--nocapture"],
                cwd=source,
                stdout=stdout,
                stderr=stderr,
                start_new_session=True,
            )
            timed_out = False
            try:
                exit_code = process.wait(timeout=900)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    exit_code = process.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    exit_code = process.wait()
        result = {
            "producer": name,
            "process_exit": exit_code,
            "timed_out": timed_out,
            "wall_seconds": time.monotonic() - started,
            "stdout_sha256": sha(root / (name + ".stdout")),
            "stderr_sha256": sha(root / (name + ".stderr")),
        }
        report["records"].append(result)
        save()
        print(name, result, flush=True)
        assert sha(Path(p["binary"])) == p["binary_sha256"]
    report["terminal"] = len(report["records"]) == 2
    save()
    raise SystemExit(
        0
        if all(r["process_exit"] == 0 and not r["timed_out"] for r in report["records"])
        else 1
    )


if __name__ == "__main__":
    main()

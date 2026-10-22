#!/usr/bin/env python3
# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0
"""Compare pinned PostgreSQL 18.0 cases against the embedded HawDB worker."""

import argparse
import collections
import ctypes
import ctypes.util
import hashlib
import json
import math
import os
from pathlib import Path
import select
import subprocess
import sys
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]
REPO = ROOT.parents[1]
UPSTREAM_SHA = "3d6a828938a5fa0444275d3d2f67b64ec3199eb7"


class InfrastructureError(Exception):
    pass


def file_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1048576), b""):
            digest.update(chunk)
    return digest.hexdigest()


def canonical_cell(oid, value):
    if value is None:
        return {"kind": "null"}
    if oid == 16:
        if value not in ("t", "f"):
            raise InfrastructureError("invalid boolean from libpq")
        return {"kind": "bool", "value": value == "t"}
    if oid in (20, 21, 23):
        return {"kind": "int", "value": str(int(value))}
    if oid in (700, 701):
        return {"kind": "float", "value": value}
    if oid in (18, 19, 25, 1042, 1043):
        return {"kind": "text", "value": value}
    return {"kind": f"pg_oid_{oid}", "value": value}


def normalized_rows(response, ordered):
    rows = []
    for row in response["rows"]:
        cells = []
        for cell in row:
            cell = dict(cell)
            if cell["kind"] == "float":
                value = float(cell["value"])
                cell["value"] = "nan" if math.isnan(value) else value.hex()
            cells.append(cell)
        rows.append(json.dumps(cells, sort_keys=True, ensure_ascii=False))
    # A multiset preserves duplicate multiplicity. Never sort ordered results.
    return rows if ordered else sorted(rows)


def equivalent(pg, hawdb, step):
    expected_error = step.get("expected_error")
    if expected_error:
        return (
            pg["status"] == hawdb["status"] == "error"
            and pg.get("sqlstate") == expected_error["pg_sqlstate"]
            and hawdb.get("kind") == expected_error["hawdb_kind"]
        )
    if pg["status"] != "ok" or hawdb["status"] != "ok":
        return False
    if step.get("compare_column_names", True):
        # The embedded API returns named rows; compare the name/value mapping,
        # not PostgreSQL's projection order against its canonical key order.
        if sorted(pg["columns"]) != sorted(hawdb["columns"]):
            return False
        if len(set(pg["columns"])) != len(pg["columns"]):
            return False

        def named(response):
            indices = sorted(
                range(len(response["columns"])), key=lambda i: response["columns"][i]
            )
            return dict(
                response, rows=[[row[i] for i in indices] for row in response["rows"]]
            )

        pg, hawdb = named(pg), named(hawdb)
    return (
        len(pg["columns"]) == len(hawdb["columns"])
        and pg["affected"] == hawdb["affected"]
        and normalized_rows(pg, step.get("ordered", False))
        == normalized_rows(hawdb, step.get("ordered", False))
    )


class Libpq:
    def __init__(self, library):
        path = library or ctypes.util.find_library("pq")
        if not path:
            raise InfrastructureError("libpq not found; pass --libpq /path/to/libpq")
        self.lib = ctypes.CDLL(path)
        signatures = {
            "PQconnectdbParams": (
                ctypes.c_void_p,
                [
                    ctypes.POINTER(ctypes.c_char_p),
                    ctypes.POINTER(ctypes.c_char_p),
                    ctypes.c_int,
                ],
            ),
            "PQstatus": (ctypes.c_int, [ctypes.c_void_p]),
            "PQserverVersion": (ctypes.c_int, [ctypes.c_void_p]),
            "PQerrorMessage": (ctypes.c_char_p, [ctypes.c_void_p]),
            "PQfinish": (None, [ctypes.c_void_p]),
            "PQexec": (ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p]),
            "PQresultStatus": (ctypes.c_int, [ctypes.c_void_p]),
            "PQresultErrorField": (ctypes.c_char_p, [ctypes.c_void_p, ctypes.c_int]),
            "PQntuples": (ctypes.c_int, [ctypes.c_void_p]),
            "PQnfields": (ctypes.c_int, [ctypes.c_void_p]),
            "PQfname": (ctypes.c_char_p, [ctypes.c_void_p, ctypes.c_int]),
            "PQftype": (ctypes.c_uint, [ctypes.c_void_p, ctypes.c_int]),
            "PQgetisnull": (
                ctypes.c_int,
                [ctypes.c_void_p, ctypes.c_int, ctypes.c_int],
            ),
            "PQgetvalue": (
                ctypes.c_void_p,
                [ctypes.c_void_p, ctypes.c_int, ctypes.c_int],
            ),
            "PQgetlength": (
                ctypes.c_int,
                [ctypes.c_void_p, ctypes.c_int, ctypes.c_int],
            ),
            "PQcmdTuples": (ctypes.c_char_p, [ctypes.c_void_p]),
            "PQcmdStatus": (ctypes.c_char_p, [ctypes.c_void_p]),
            "PQclear": (None, [ctypes.c_void_p]),
        }
        for name, (result, args) in signatures.items():
            function = getattr(self.lib, name)
            function.restype = result
            function.argtypes = args


class PgConnection:
    def __init__(self, api, dsn):
        self.lib = api.lib
        keywords = (ctypes.c_char_p * 3)(b"dbname", b"connect_timeout", None)
        values = (ctypes.c_char_p * 3)(dsn.encode(), b"5", None)
        self.conn = self.lib.PQconnectdbParams(keywords, values, 1)
        if not self.conn:
            raise InfrastructureError("libpq could not allocate a connection")
        if self.lib.PQstatus(self.conn) != 0:
            self.close()
            raise InfrastructureError(
                "PostgreSQL connection failed; check PGREGRESS_DSN"
            )
        version = self.lib.PQserverVersion(self.conn)
        if version != 180000:
            self.close()
            raise InfrastructureError(
                f"expected PostgreSQL 18.0 (180000), got {version}"
            )
        for sql in (
            "SET client_encoding = 'UTF8'",
            "SET timezone = 'UTC'",
            "SET datestyle = 'ISO, YMD'",
            "SET extra_float_digits = 3",
            "SET statement_timeout = '3000ms'",
            "SET lock_timeout = '500ms'",
            "SET client_min_messages = warning",
        ):
            if self.execute(sql)["status"] != "ok":
                self.close()
                raise InfrastructureError("failed to initialize PostgreSQL session")

    def execute(self, sql):
        result = self.lib.PQexec(self.conn, sql.encode())
        if not result:
            raise InfrastructureError("libpq returned no result")
        try:
            status = self.lib.PQresultStatus(result)
            if status not in (1, 2):
                code = self.lib.PQresultErrorField(result, ord("C"))
                message = self.lib.PQresultErrorField(result, ord("M"))
                if self.lib.PQstatus(self.conn) != 0:
                    raise InfrastructureError("PostgreSQL connection lost")
                if code and code.decode() in ("57014", "55P03"):
                    raise InfrastructureError(
                        "reference statement/lock timeout; wait-aware schedules are unsupported"
                    )
                return {
                    "status": "error",
                    "sqlstate": code.decode() if code else None,
                    "message": (
                        message.decode() if message else "unknown PostgreSQL error"
                    ),
                }
            columns = [
                self.lib.PQfname(result, i).decode()
                for i in range(self.lib.PQnfields(result))
            ]
            types = [self.lib.PQftype(result, i) for i in range(len(columns))]
            rows = []
            count = self.lib.PQntuples(result)
            if count > 4096:
                raise InfrastructureError(
                    "reference result exceeds 4096-row fixture limit"
                )
            for row in range(count):
                values = []
                for col, oid in enumerate(types):
                    value = None
                    if not self.lib.PQgetisnull(result, row, col):
                        length = self.lib.PQgetlength(result, row, col)
                        if length > 1048576:
                            raise InfrastructureError(
                                "reference cell exceeds 1 MiB fixture limit"
                            )
                        value = ctypes.string_at(
                            self.lib.PQgetvalue(result, row, col), length
                        ).decode()
                    values.append(canonical_cell(oid, value))
                rows.append(values)
            tag = self.lib.PQcmdStatus(result).decode()
            affected = self.lib.PQcmdTuples(result).decode()
            return {
                "status": "ok",
                "columns": columns,
                "rows": rows,
                "affected": (
                    int(affected or 0)
                    if tag.split()[0] in ("INSERT", "UPDATE", "DELETE")
                    else 0
                ),
                "pg_types": types,
                "command_tag": tag,
            }
        finally:
            self.lib.PQclear(result)

    def close(self):
        if self.conn:
            self.lib.PQfinish(self.conn)
            self.conn = None


class Worker:
    def __init__(self, binary, timeout):
        self.timeout = timeout
        self.process = subprocess.Popen(
            [str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE
        )
        self.buffer = b""

    def execute(self, request):
        self.process.stdin.write(json.dumps(request).encode() + b"\n")
        self.process.stdin.flush()
        deadline = time.monotonic() + self.timeout
        while b"\n" not in self.buffer:
            remaining = deadline - time.monotonic()
            if (
                remaining <= 0
                or not select.select([self.process.stdout], [], [], remaining)[0]
            ):
                raise InfrastructureError("HawDB worker timed out")
            chunk = os.read(self.process.stdout.fileno(), 65536)
            if not chunk:
                raise InfrastructureError("HawDB worker exited without a result")
            self.buffer += chunk
            if len(self.buffer) > 8 * 1048576:
                raise InfrastructureError("HawDB response exceeds 8 MiB fixture limit")
        line, self.buffer = self.buffer.split(b"\n", 1)
        return json.loads(line)

    def close(self):
        try:
            self.process.stdin.close()
        except BrokenPipeError:
            pass
        try:
            self.process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        self.process.stdout.close()


def load_suite(root=ROOT):
    suite = json.loads((root / "data/cases.json").read_text())
    if (
        suite["format_version"] != 1
        or suite["postgres_commit"] != UPSTREAM_SHA
        or suite["postgres_version"] != 180000
    ):
        raise InfrastructureError("suite must pin PostgreSQL REL_18_0")
    sources = {}
    for source in suite["sources"]:
        path = Path(source["file"])
        if path.is_absolute() or ".." in path.parts:
            raise InfrastructureError("source path must stay inside the test project")
        content = (root / path).read_bytes()
        if hashlib.sha256(content).hexdigest() != source["sha256"]:
            raise InfrastructureError(f"source checksum mismatch: {path}")
        sources[str(path)] = content.decode()
    ids = set()
    for case in suite["cases"]:
        if case["id"] in ids:
            raise InfrastructureError("duplicate case ID")
        ids.add(case["id"])
        if case.get("unsupported"):
            continue
        for step in case["steps"]:
            ref = step["source"]
            lines = sources[ref["file"]].splitlines(keepends=True)
            if not 1 <= ref["first_line"] <= ref["last_line"] <= len(lines):
                raise InfrastructureError("invalid source line range")
            original = "".join(lines[ref["first_line"] - 1 : ref["last_line"]])
            action = step.get("action", "execute")
            control = step["sql"].strip().rstrip(";").upper()
            if action == "begin":
                isolation = step.get("isolation")
                suffix = {
                    "repeatable_read": "REPEATABLE READ",
                    "serializable": "SERIALIZABLE",
                }.get(isolation)
                if not suffix or control not in (
                    f"BEGIN ISOLATION LEVEL {suffix}",
                    f"BEGIN TRANSACTION ISOLATION LEVEL {suffix}",
                ):
                    raise InfrastructureError(
                        "source SQL and mapped transaction isolation differ"
                    )
            elif action in ("commit", "rollback"):
                if control != action.upper():
                    raise InfrastructureError(
                        "source SQL and transaction action differ"
                    )
            elif action != "execute":
                raise InfrastructureError("unsupported manifest action")
            if "execute_sql" in step and (
                not step.get("adaptation")
                or not step["sql"]
                .lstrip()
                .upper()
                .startswith(("CREATE TABLE", "INSERT INTO"))
            ):
                raise InfrastructureError(
                    "fixture SQL adaptation requires explicit setup provenance"
                )
            if step["sql"] not in original:
                raise InfrastructureError(f"SQL changed from upstream in {case['id']}")
            if not step.get("compare_column_names", True) and not step.get(
                "adaptation"
            ):
                raise InfrastructureError(
                    "column label adaptation requires an explanation"
                )
    return suite


def selected_cases(suite, schedule, pattern):
    groups = []
    for line in schedule.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        if not line.startswith("test:") or len(line.split()) != 2:
            raise InfrastructureError(
                "schedule requires one serial 'test: GROUP' per line"
            )
        group = line.split()[1]
        if group in groups or not any(c["group"] == group for c in suite["cases"]):
            raise InfrastructureError(f"duplicate or unknown schedule group: {group}")
        groups.append(group)
    cases = [
        c
        for group in groups
        for c in suite["cases"]
        if c["group"] == group and (not pattern or pattern in c["id"])
    ]
    if not cases:
        raise InfrastructureError("selection matched zero cases")
    return cases


def run_case(case, api, dsn, worker):
    if case.get("unsupported"):
        return {
            "id": case["id"],
            "status": "UNSUPPORTED",
            "reason": case["unsupported"],
            "trace": [],
        }
    result = {"id": case["id"], "status": "PASS", "trace": []}
    connections = {}
    namespace = "hawdb_pg18_" + uuid.uuid4().hex
    admin = PgConnection(api, dsn)
    created = False
    try:
        if admin.execute(f'CREATE SCHEMA "{namespace}"')["status"] != "ok":
            raise InfrastructureError("could not create isolated test schema")
        created = True
        if worker.execute({"action": "reset"})["status"] != "ok":
            raise InfrastructureError("HawDB reset failed")
        for step in case["steps"]:
            session = step.get("session", "main")
            if session not in connections:
                connection = PgConnection(api, dsn)
                connections[session] = connection
                if (
                    connection.execute(f'SET search_path TO "{namespace}"')["status"]
                    != "ok"
                ):
                    raise InfrastructureError("could not select test schema")
            effective_sql = step.get("execute_sql", step["sql"])
            pg = connections[session].execute(effective_sql)
            request = {"action": step.get("action", "execute"), "session": session}
            if request["action"] == "execute":
                request["sql"] = effective_sql
            elif request["action"] == "begin":
                request["isolation"] = step["isolation"]
            hawdb = worker.execute(request)
            result["trace"].append({"step": step, "postgres": pg, "hawdb": hawdb})
            if not equivalent(pg, hawdb, step):
                result["status"] = "MISMATCH"
                result["reason"] = (
                    "first differing statement; remaining steps not executed"
                )
                break
    finally:
        for connection in connections.values():
            connection.close()
        try:
            if (
                created
                and admin.execute(f'DROP SCHEMA "{namespace}" CASCADE')["status"]
                != "ok"
            ):
                raise InfrastructureError(
                    f"cleanup failed; retained test schema {namespace}"
                )
        finally:
            admin.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dsn-env",
        default="PGREGRESS_DSN",
        help="environment variable containing libpq conninfo",
    )
    parser.add_argument("--libpq", help="path to a libpq shared library")
    parser.add_argument(
        "--worker",
        type=Path,
        help="prebuilt Cargo/Bazel worker (otherwise build with Cargo)",
    )
    parser.add_argument(
        "--case", default="", help="case ID substring; zero matches fail"
    )
    parser.add_argument("--schedule", type=Path, default=ROOT / "data/schedule")
    parser.add_argument(
        "--report", type=Path, default=REPO / "target/pg-regress/report.json"
    )
    parser.add_argument("--worker-timeout", type=float, default=10)
    parser.add_argument(
        "--list",
        action="store_true",
        help="validate provenance and list cases without a database",
    )
    args = parser.parse_args()
    suite = load_suite()
    cases = selected_cases(suite, args.schedule, args.case)
    if args.list:
        for case in cases:
            print(
                f"{'UNSUPPORTED' if case.get('unsupported') else 'READY':11} {case['id']}"
            )
        return 0
    report = {
        "postgres_version": 180000,
        "postgres_commit": UPSTREAM_SHA,
        "sources": suite["sources"],
        "harness_sha256": {
            (
                str(path.relative_to(ROOT))
                if path.is_relative_to(ROOT)
                else "selected_schedule"
            ): file_digest(path)
            for path in [
                ROOT / "src/main.rs",
                ROOT / "src/runner.py",
                ROOT / "Cargo.toml",
                ROOT / "Cargo.lock",
                ROOT / "data/cases.json",
                args.schedule,
            ]
        },
        "cases": [],
        "selection": args.case,
        "selected_cases": len(cases),
        "completed": False,
        "hawdb_revision": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], cwd=REPO, text=True
        ).strip(),
        "working_tree_dirty": bool(
            subprocess.check_output(["git", "status", "--porcelain"], cwd=REPO)
        ),
    }
    worker = None
    try:
        if args.worker_timeout <= 0:
            raise InfrastructureError("worker timeout must be positive")
        dsn = os.environ.get(args.dsn_env)
        if not dsn:
            raise InfrastructureError(
                f"set {args.dsn_env} to a PostgreSQL 18.0 libpq conninfo"
            )
        api = Libpq(args.libpq)
        # Refuse the wrong server before compiling or executing any HawDB case.
        probe = PgConnection(api, dsn)
        probe.close()
        binary = args.worker
        if binary is None:
            subprocess.run(
                [
                    "cargo",
                    "build",
                    "--locked",
                    "--manifest-path",
                    str(ROOT / "Cargo.toml"),
                ],
                cwd=REPO,
                check=True,
            )
            metadata = json.loads(
                subprocess.check_output(
                    [
                        "cargo",
                        "metadata",
                        "--locked",
                        "--no-deps",
                        "--format-version",
                        "1",
                        "--manifest-path",
                        str(ROOT / "Cargo.toml"),
                    ],
                    cwd=REPO,
                )
            )
            binary = (
                Path(metadata["target_directory"]) / "debug/hawdb-pg-regress-worker"
            )
        report["worker_sha256"] = file_digest(binary)
        worker = Worker(binary, args.worker_timeout)
        for case in cases:
            report["active_case"] = case["id"]
            result = run_case(case, api, dsn, worker)
            report["cases"].append(result)
            print(
                f"{result['status']:11} {case['id']} ({len(result['trace'])} statements)",
                flush=True,
            )
            if result["status"] == "UNSUPPORTED":
                print(f"  Reason: {result['reason']}", flush=True)
            if result["status"] == "MISMATCH":
                last = result["trace"][-1]
                print(
                    f"  SQL: {last['step']['sql']}\n  PG: {last['postgres']}\n  HawDB: {last['hawdb']}",
                    flush=True,
                )
        report.pop("active_case", None)
        report["completed"] = True
    except (
        InfrastructureError,
        OSError,
        ValueError,
        subprocess.SubprocessError,
        KeyboardInterrupt,
    ) as error:
        report["infrastructure_error"] = str(error) or "interrupted"
        print(f"INFRA_ERROR: {error}", file=sys.stderr)
    finally:
        if worker:
            worker.close()
        report["counts"] = dict(
            collections.Counter(c["status"] for c in report["cases"])
        )
        args.report.parent.mkdir(parents=True, exist_ok=True)
        args.report.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(f"SUMMARY {report['counts']} | report: {args.report}")
    if "infrastructure_error" in report:
        return 2
    if report["counts"].get("MISMATCH") or not report["counts"].get("PASS"):
        return 1
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (InfrastructureError, OSError, ValueError) as error:
        print(f"INFRA_ERROR: {error}", file=sys.stderr)
        sys.exit(2)

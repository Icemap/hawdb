# Copyright 2026 Nowledge
# SPDX-License-Identifier: Apache-2.0

"""Engine read ceilings and plan-only execution through the Python facade."""

import pytest

import hawdb
from hawdb import exceptions


@pytest.fixture(params=[hawdb.open, hawdb.Database], ids=["open", "constructor"])
def opener(request):
    return request.param


@pytest.fixture(params=["memory", "file"])
def database_path(request, tmp_path):
    return None if request.param == "memory" else tmp_path / "graph"


def seed(db):
    for value in [1, 2, 3]:
        db.execute("CREATE (:Item {id: $id})", {"id": value})


def test_row_ceiling_refuses_without_poisoning_handle(opener, database_path):
    with opener(database_path, max_read_result_rows=1) as db:
        seed(db)
        with pytest.raises(exceptions.ExecutionError, match="max_read_result_rows"):
            db.execute("MATCH (n:Item) RETURN n.id AS id ORDER BY id")
        assert db.execute(
            "MATCH (n:Item) RETURN n.id AS id ORDER BY id LIMIT 1"
        ).fetchall() == [{"id": 1}]


def test_payload_ceiling_refuses_then_accepts_small_result(opener, database_path):
    with opener(database_path, max_read_result_payload_bytes=128) as db:
        with pytest.raises(
            exceptions.ExecutionError, match="max_read_result_payload_bytes"
        ):
            db.execute("RETURN $text AS text", {"text": "x" * 1024})
        assert db.execute("RETURN 'ok' AS text").fetchall() == [{"text": "ok"}]


def test_omitted_and_none_budgets_preserve_defaults(opener, database_path):
    for options in [
        {},
        {"max_read_result_rows": None, "max_read_result_payload_bytes": None},
    ]:
        with opener(database_path, **options) as db:
            assert db.execute("UNWIND [1, 2, 3] AS id RETURN id").fetchall() == [
                {"id": 1}, {"id": 2}, {"id": 3}
            ]
            result = db.execute("RETURN $text AS text", {"text": "x" * 1024})
            assert result.fetchall() == [
                {"text": "x" * 1024}
            ]


def test_read_only_open_applies_both_ceilings(opener, tmp_path):
    path = tmp_path / "read-only"
    with hawdb.open(path) as writer:
        seed(writer)
    with opener(
        path, read_only=True, max_read_result_rows=1, max_read_result_payload_bytes=128
    ) as db:
        with pytest.raises(exceptions.ExecutionError, match="max_read_result_rows"):
            db.execute("MATCH (n:Item) RETURN n.id AS id")
        with pytest.raises(
            exceptions.ExecutionError, match="max_read_result_payload_bytes"
        ):
            db.execute("RETURN $text AS text", {"text": "x" * 1024})
        result = db.execute("MATCH (n:Item) RETURN n.id AS id ORDER BY id LIMIT 1")
        assert result.fetchone() == {"id": 1}


def test_open_time_budget_is_not_persisted(tmp_path):
    path = tmp_path / "reopen"
    with hawdb.open(path, max_read_result_rows=1) as db:
        seed(db)
    with hawdb.open(path) as db:
        assert len(db.execute("MATCH (n:Item) RETURN n.id")) == 3


def test_zero_ceilings_follow_engine_semantics():
    with hawdb.open(max_read_result_rows=0) as db:
        assert len(db.execute("UNWIND [1, 2] AS id RETURN id")) == 2
    with hawdb.open(max_read_result_payload_bytes=0) as db:
        with pytest.raises(exceptions.ExecutionError):
            db.execute("RETURN 'nonempty' AS value")


@pytest.mark.parametrize(
    "option", ["max_read_result_rows", "max_read_result_payload_bytes"]
)
@pytest.mark.parametrize(
    "value,error", [(-1, OverflowError), (2**100, OverflowError), ("1", TypeError)]
)
def test_invalid_budget_is_rejected_before_open(opener, tmp_path, option, value, error):
    path = tmp_path / "invalid"
    with pytest.raises(error):
        opener(path, **{option: value})
    assert not path.exists()


def test_sql_reads_share_the_open_time_ceiling(database_path):
    with hawdb.open(database_path, max_read_result_rows=1) as db:
        db.execute_sql("CREATE TABLE items (id BIGINT PRIMARY KEY)")
        db.execute_sql("INSERT INTO items (id) VALUES (1), (2)")
        with pytest.raises(exceptions.ExecutionError, match="max_read_result_rows"):
            db.execute_sql("SELECT id FROM items")
        result = db.execute_sql("SELECT id FROM items ORDER BY id LIMIT 1")
        assert result.fetchall() == [{"id": 1}]


def test_sql_explain_still_uses_execute_sql(database_path):
    with hawdb.open(database_path) as db:
        db.execute_sql("CREATE TABLE items (id BIGINT PRIMARY KEY)")
        assert db.execute_sql("EXPLAIN SELECT id FROM items").fetchall()


def test_explain_binds_parameters_without_executing(database_path):
    with hawdb.open(database_path, max_read_result_rows=1) as db:
        seed(db)
        plan = db.explain("MATCH (n:Item) WHERE n.id = $id RETURN n.id", {"id": 2})
        assert isinstance(plan, str) and plan.strip()
        assert db.explain("CREATE (:PlannedOnly {id: $id})", {"id": 9}).strip()
        assert db.execute("MATCH (n:PlannedOnly) RETURN n.id").fetchall() == []
        assert db.execute("MATCH (n:Item) RETURN count(n) AS count").fetchall() == [
            {"count": 3}
        ]


def test_explain_preserves_error_and_handle_contracts():
    db = hawdb.open()
    with pytest.raises(TypeError):
        db.explain("RETURN $value", {"value": object()})
    with pytest.raises(exceptions.ParseError):
        db.explain("MATCH (")
    with db.transaction():
        with pytest.raises(RuntimeError, match="transaction is open"):
            db.explain("RETURN 1")
    db.close()
    with pytest.raises(RuntimeError, match="database is closed"):
        db.explain("RETURN 1")

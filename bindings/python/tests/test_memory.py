# Copyright 2026 Nowledge
#
# Licensed under the Apache License, Version 2.0 (the "License");
# you may not use this file except in compliance with the License.
# You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

import pytest

import hawdb


def test_memory_open_creates_and_matches():
    db = hawdb.open()
    assert db.is_open
    assert db.path is None
    db.execute("CREATE (:Memory {id: $id, title: $title})", {"id": 1, "title": "ad-hoc"})
    assert db.execute("MATCH (m:Memory) RETURN m.id AS id, m.title AS title").fetchall() == [
        {"id": 1, "title": "ad-hoc"}
    ]
    db.execute_sql("CREATE TABLE items (id BIGINT PRIMARY KEY)")
    db.execute_sql("INSERT INTO items (id) VALUES ($1)", [42])
    assert db.execute_sql("SELECT id FROM items").fetchall() == [{"id": 42}]
    db.close()


def test_memory_open_drops_data_on_close():
    db = hawdb.open()
    db.execute("CREATE (:Memory {id: 1})")
    db.close()

    again = hawdb.open()
    try:
        assert again.execute("MATCH (m:Memory) RETURN m.id AS id").fetchall() == []
    finally:
        again.close()


def test_memory_databases_do_not_share_data():
    first = hawdb.open()
    second = hawdb.open()
    try:
        first.execute("CREATE (:Memory {id: 1})")
        assert second.execute("MATCH (m:Memory) RETURN m.id AS id").fetchall() == []
    finally:
        first.close()
        second.close()


def test_memory_open_rejects_read_only():
    with pytest.raises(ValueError):
        hawdb.open(read_only=True)
    with pytest.raises(ValueError):
        hawdb.Database(read_only=True)


@pytest.mark.parametrize(
    "opener", [hawdb.open, hawdb.Database, hawdb.connect], ids=["open", "Database", "connect"]
)
@pytest.mark.parametrize("read_only", [False, True])
def test_memory_sentinel_path_is_refused(tmp_path, monkeypatch, opener, read_only):
    # sqlite3 spells an in-memory database ":memory:"; HawDB takes no path.
    monkeypatch.chdir(tmp_path)
    with pytest.raises(ValueError, match="pass no path for an in-memory database"):
        with opener(":memory:", read_only=read_only):
            pass
    assert list(tmp_path.iterdir()) == []


@pytest.mark.parametrize(
    "opener", [hawdb.open, hawdb.Database, hawdb.connect], ids=["open", "Database", "connect"]
)
@pytest.mark.parametrize("read_only", [False, True])
@pytest.mark.parametrize("existing_project", [False, True])
def test_empty_path_is_refused(
    tmp_path, monkeypatch, opener, read_only, existing_project
):
    if existing_project:
        with hawdb.open(tmp_path) as db:
            db.execute("CREATE (:Persisted {id: 9})")
    monkeypatch.chdir(tmp_path)
    before = sorted(tmp_path.rglob("*"))
    with pytest.raises(ValueError, match="path is empty; pass no path"):
        with opener("", read_only=read_only):
            pass
    assert sorted(tmp_path.rglob("*")) == before
    if existing_project:
        with hawdb.open(tmp_path, read_only=True) as db:
            assert db.execute("MATCH (p:Persisted) RETURN p.id AS id").fetchall() == [
                {"id": 9}
            ]


def test_database_constructor_without_path_opens_memory():
    db = hawdb.Database()
    try:
        assert db.path is None
        assert "in-memory" in repr(db)
    finally:
        db.close()

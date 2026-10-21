// Copyright 2026 Nowledge
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use super::*;

fn setup(database: &mut Database) {
    let mut session = database.sql_session();
    session.query(
        "CREATE TABLE memories (id TEXT PRIMARY KEY, space_id TEXT NOT NULL, embedding VECTOR(1024))", &[],
    ).unwrap();
    session
        .query(
            "INSERT INTO memories (id, space_id) VALUES ('a', 'default')",
            &[],
        )
        .unwrap();
    session
        .query(
            "ALTER TABLE memories ADD COLUMN embedding_v2 VECTOR(768) DEFAULT NULL",
            &[],
        )
        .unwrap();
    session
        .query("CREATE INDEX memory_scope ON memories (space_id)", &[])
        .unwrap();
}

fn assert_schema_and_rows(database: &mut Database) {
    let mut session = database.sql_session();
    let columns = session.query(
        "SELECT column_name, dimensions, element_type, nullable FROM system.vector_columns WHERE table_name = 'memories' ORDER BY column_name", &[],
    ).unwrap().output.rows;
    assert_eq!(columns.len(), 2);
    for (column, (name, dimension)) in columns
        .iter()
        .zip([("embedding", 1024), ("embedding_v2", 768)])
    {
        assert_eq!(column["column_name"], Value::String(name.into()));
        assert_eq!(column["dimensions"], Value::Int(dimension));
        assert_eq!(column["element_type"], Value::String("float32".into()));
        assert_eq!(column["nullable"], Value::Bool(true));
    }
    let columns = session.query(
        "SELECT data_type, udt_name, udt_schema FROM information_schema.columns WHERE table_name = 'memories' AND column_name = 'embedding'", &[],
    ).unwrap().output.rows;
    assert_eq!(columns.len(), 1);
    assert_eq!(
        columns[0]["data_type"],
        Value::String("USER-DEFINED".into())
    );
    assert_eq!(columns[0]["udt_name"], Value::String("vector".into()));
    assert_eq!(columns[0]["udt_schema"], Value::String("public".into()));
    let rows = session.query(
        "SELECT id, embedding, embedding_v2 FROM memories WHERE space_id = 'default' AND embedding IS NULL ORDER BY id", &[],
    ).unwrap().output.rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], Value::String("a".into()));
    assert_eq!(rows[0]["embedding"], Value::Null);
    assert_eq!(rows[0]["embedding_v2"], Value::Null);
}

#[test]
fn nullable_vectors_survive_wal_and_checkpoint_reopen() {
    for checkpoint in [false, true] {
        let root = std::env::temp_dir().join(format!(
            "hawdb-vector-schema-{}-{}-{checkpoint}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        {
            let mut database = Database::open(&root).unwrap();
            setup(&mut database);
            assert_schema_and_rows(&mut database);
            if checkpoint {
                database.checkpoint().unwrap();
            }
        }
        {
            let mut database = Database::open(&root).unwrap();
            assert_schema_and_rows(&mut database);
            database
                .sql_session()
                .query("UPDATE memories SET embedding = NULL WHERE id = 'a'", &[])
                .unwrap();
        }
        {
            let mut database = Database::open_with_config(
                &root,
                crate::DatabaseConfig {
                    read_only: true,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_schema_and_rows(&mut database);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn unsupported_vector_writes_and_constraints_leave_no_partial_state() {
    let mut database = Database::new();
    setup(&mut database);
    for sql in [
        "CREATE TABLE invalid (id VECTOR(3) PRIMARY KEY)",
        "CREATE TABLE invalid (id TEXT PRIMARY KEY, v VECTOR(3) UNIQUE)",
        "CREATE TABLE invalid (id TEXT PRIMARY KEY, v VECTOR(3) DEFAULT '1,2,3')",
        "CREATE TABLE invalid (id TEXT PRIMARY KEY, v VECTOR(3) REFERENCES memories (id))",
        "CREATE INDEX invalid ON memories (embedding)",
        "CREATE UNIQUE INDEX invalid ON memories (embedding)",
        "ALTER TABLE memories ADD COLUMN invalid VECTOR(0)",
        "ALTER TABLE memories ADD COLUMN invalid VECTOR(3) NOT NULL",
        "INSERT INTO memories (id, space_id, embedding) VALUES ('b', 'default', '1,2,3')",
        "UPDATE memories SET embedding = 1 WHERE id = 'a'",
        "CREATE TABLE invalid (stream TEXT NOT NULL, seq BIGINT NOT NULL, v VECTOR(3)) WITH (storage_mode = 'strict_append', partition_key = 'stream', order_key = 'seq')",
    ] {
        let before = database.commit_epoch().unwrap();
        assert!(database.sql_session().transaction(|transaction| {
            transaction.query("INSERT INTO memories (id, space_id) VALUES ('rollback', 'default')", &[])?;
            transaction.query(sql, &[])?;
            Ok(())
        }).is_err(), "{sql}");
        assert_eq!(database.commit_epoch().unwrap(), before, "{sql}");
        assert_schema_and_rows(&mut database);
    }
    for value in [
        Value::List(vec![Value::Float(1.0); 1024]),
        Value::Binary(vec![0; 4096]),
    ] {
        assert!(database
            .sql_session()
            .query(
                "UPDATE memories SET embedding = $1 WHERE id = 'a'",
                &[value],
            )
            .is_err());
    }
    assert_schema_and_rows(&mut database);
    database
        .sql_session()
        .query(
            "CREATE TABLE required (id TEXT PRIMARY KEY, v VECTOR(3) NOT NULL)",
            &[],
        )
        .unwrap();
    assert!(database
        .sql_session()
        .query("INSERT INTO required (id) VALUES ('a')", &[])
        .is_err());
}

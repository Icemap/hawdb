// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use hawdb_storage::relational::{
    RelationalMutationLimits, RelationalOverflowConfig, RelationalValue,
};

fn limits(rows: usize) -> RelationalMutationLimits {
    RelationalMutationLimits {
        max_rows: NonZeroUsize::new(rows).unwrap(),
        max_payload_bytes: NonZeroUsize::new(1024 * 1024).unwrap(),
    }
}

fn extrema_base(db: &mut Database) {
    base(db);
    db.query_sql("CREATE INDEX by_bucket ON events (bucket, amount)")
        .unwrap();
}

fn assert_extrema_matches(db: &mut Database, grouped: bool) {
    let grouping = if grouped { " GROUP BY bucket" } else { "" };
    let bucket = if grouped { "bucket, " } else { "" };
    let expected = db.query_sql(&format!("SELECT {bucket}COUNT(*) AS n, COUNT(amount) AS present, SUM(amount) AS total, MIN(amount) AS lo, MAX(amount) AS hi FROM events WHERE enabled = true{grouping}")).unwrap();
    let actual = db.query_sql("SELECT * FROM stats").unwrap();
    let normalize = |output: QueryOutput| {
        let mut rows = output
            .rows
            .iter()
            .map(|row| format!("{row:?}"))
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    assert_eq!(normalize(actual), normalize(expected));
}

#[test]
fn materialized_extrema_retract_duplicates_nulls_filters_and_group_moves() {
    for grouped in [false, true] {
        let mut db = Database::new();
        extrema_base(&mut db);
        let grouping = if grouped { " GROUP BY bucket" } else { "" };
        let bucket = if grouped { "bucket, " } else { "" };
        db.query_sql(&format!("CREATE MATERIALIZED VIEW stats AS SELECT {bucket}COUNT(*) AS n, COUNT(amount) AS present, SUM(amount) AS total, MIN(amount) AS lo, MAX(amount) AS hi FROM events WHERE enabled = true{grouping}")).unwrap();
        assert_extrema_matches(&mut db, grouped);
        for statement in [
            "INSERT INTO events (id, bucket, amount, enabled) VALUES (1, 'a', 10, true), (2, 'a', 10, true), (3, 'a', 20, true), (4, NULL, NULL, true), (5, 'b', -8, false)",
            "DELETE FROM events WHERE id = 1",
            "UPDATE events SET amount = 30 WHERE id = 2",
            "UPDATE events SET bucket = NULL, amount = -2 WHERE id = 3",
            "UPDATE events SET enabled = true WHERE id = 5",
            "UPDATE events SET id = 6, bucket = 'b', amount = 4 WHERE id = 2",
            "UPDATE events SET amount = NULL WHERE id = 5",
            "UPDATE events SET enabled = false WHERE id = 3",
            "DELETE FROM events WHERE enabled = true",
        ] {
            db.query_sql(statement).unwrap();
            assert_extrema_matches(&mut db, grouped);
        }
    }
}

#[test]
fn materialized_extrema_backfill_and_updates_use_only_affected_index_prefixes() {
    let mut db = Database::new();
    extrema_base(&mut db);
    db.query_sql("INSERT INTO events (id, bucket, amount, enabled) VALUES (1, 'a', 1, true), (2, 'a', 2, true)")
        .unwrap();
    for id in 3..103 {
        db.query_sql_with_params(
            "INSERT INTO events (id, bucket, amount, enabled) VALUES ($1, 'other', $1, true)",
            &[Value::Int(id)],
        )
        .unwrap();
    }
    db.query_sql("CREATE MATERIALIZED VIEW stats AS SELECT bucket, MIN(amount) AS lo, MAX(amount) AS hi FROM events GROUP BY bucket").unwrap();
    let state = db.runtime.get().unwrap().store.relational_state().clone();
    let update = crate::relational_sql::compile_relational_statement_sql(
        "UPDATE events SET amount = 5 WHERE id = 1",
        &[],
        &state,
    )
    .unwrap();
    // Old/new changed row, two prefix entries, old view row, output row.
    let staged = state
        .stage_transaction(
            update.clone(),
            limits(6),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    let row = staged
        .rows("stats")
        .find(|(_, row)| row.values()[0] == RelationalValue::Text("a".into()))
        .unwrap()
        .1;
    assert_eq!(
        row.values()[1..3],
        [RelationalValue::BigInt(2), RelationalValue::BigInt(5)]
    );
    let error = state
        .stage_transaction(update, limits(5), RelationalOverflowConfig::default())
        .unwrap_err();
    assert!(error.to_string().contains("budget"), "{error}");
    assert_eq!(
        db.query_sql("SELECT lo FROM stats WHERE bucket = 'a'")
            .unwrap()
            .rows[0]["lo"],
        Value::Int(1)
    );

    // Building an output index must not spend a second backfill budget or alter counts.
    let index = crate::relational_sql::compile_relational_statement_sql(
        "CREATE INDEX stats_bucket ON stats (bucket)",
        &[],
        &state,
    )
    .unwrap();
    let indexed = state
        .stage_transaction(index, limits(1), RelationalOverflowConfig::default())
        .unwrap();
    assert_eq!(
        indexed.rows("stats").collect::<Vec<_>>(),
        state.rows("stats").collect::<Vec<_>>()
    );
}

#[test]
fn materialized_creation_budget_counts_source_reads_and_output_rows() {
    let mut db = Database::new();
    extrema_base(&mut db);
    db.query_sql(
        "INSERT INTO events (id, bucket, amount, enabled) VALUES (1, 'a', 1, true), (2, 'a', 2, true), (3, 'b', NULL, true)",
    )
    .unwrap();
    let state = db.runtime.get().unwrap().store.relational_state().clone();
    for (select, exact) in [
        ("SELECT id FROM events", 6),
        ("SELECT id FROM events WHERE amount = 1", 4),
        ("SELECT COUNT(*) AS n FROM events", 4),
        (
            "SELECT bucket, MIN(amount) AS lo FROM events GROUP BY bucket",
            5,
        ),
    ] {
        let create = crate::relational_sql::compile_relational_statement_sql(
            &format!("CREATE MATERIALIZED VIEW mv AS {select}"),
            &[],
            &state,
        )
        .unwrap();
        state
            .stage_transaction(
                create.clone(),
                limits(exact),
                RelationalOverflowConfig::default(),
            )
            .unwrap();
        assert!(state
            .stage_transaction(
                create,
                limits(exact - 1),
                RelationalOverflowConfig::default()
            )
            .is_err());
        assert!(state.table_schema("mv").is_none());
    }
    // Admission is shared across definitions in a single typed transaction.
    let mut create = crate::relational_sql::compile_relational_statement_sql(
        "CREATE MATERIALIZED VIEW a AS SELECT COUNT(*) AS n FROM events",
        &[],
        &state,
    )
    .unwrap();
    create.writes.extend(
        crate::relational_sql::compile_relational_statement_sql(
            "CREATE MATERIALIZED VIEW b AS SELECT id FROM events",
            &[],
            &state,
        )
        .unwrap()
        .writes,
    );
    state
        .stage_transaction(
            create.clone(),
            limits(10),
            RelationalOverflowConfig::default(),
        )
        .unwrap();
    assert!(state
        .stage_transaction(create, limits(9), RelationalOverflowConfig::default())
        .is_err());
}

#[test]
fn materialized_extrema_require_complete_source_indexes() {
    let mut db = Database::new();
    base(&mut db);
    let create = "CREATE MATERIALIZED VIEW stats AS SELECT bucket, MIN(amount) AS lo FROM events GROUP BY bucket";
    assert!(db
        .query_sql(create)
        .unwrap_err()
        .to_string()
        .contains("non-unique source index"));
    db.query_sql("CREATE UNIQUE INDEX incomplete ON events (bucket, amount)")
        .unwrap();
    assert!(db.query_sql(create).is_err());
    db.query_sql("CREATE INDEX wrong_prefix ON events (amount, bucket)")
        .unwrap();
    assert!(db.query_sql(create).is_err());
    db.query_sql("CREATE INDEX complete ON events (bucket, amount)")
        .unwrap();
    db.query_sql(create).unwrap();
    for select in [
        "SELECT MIN(enabled) AS lo FROM events",
        "SELECT MAX(*) AS hi FROM events",
    ] {
        assert!(db
            .query_sql(&format!("CREATE MATERIALIZED VIEW rejected AS {select}"))
            .is_err());
    }
}

#[test]
fn materialized_visible_unique_indexes_enforce_atomic_source_constraints() {
    let mut db = Database::new();
    base(&mut db);
    db.query_sql("INSERT INTO events (id, bucket, amount, enabled) VALUES (1, 'a', 5, true), (2, 'b', 5, true)")
        .unwrap();
    db.query_sql("CREATE MATERIALIZED VIEW mv AS SELECT bucket, amount FROM events")
        .unwrap();
    assert!(db
        .query_sql("CREATE UNIQUE INDEX invalid ON mv (amount)")
        .is_err());
    db.query_sql("CREATE UNIQUE INDEX unique_bucket ON mv (bucket)")
        .unwrap();
    let before = db
        .query_sql("SELECT * FROM mv ORDER BY bucket")
        .unwrap()
        .rows;
    assert!(db
        .query_sql("UPDATE events SET bucket = 'a' WHERE id = 2")
        .is_err());
    assert_eq!(
        db.query_sql("SELECT * FROM mv ORDER BY bucket")
            .unwrap()
            .rows,
        before
    );
    assert_eq!(
        db.query_sql("SELECT bucket FROM events WHERE id = 2")
            .unwrap()
            .rows[0]["bucket"],
        Value::String("b".into())
    );
    assert!(db
        .query_sql("CREATE INDEX hidden ON mv (__hawdb_mv_key)")
        .is_err());
}

// Column names, types, defaults, and indexes match Mem's content_store_v1.sql
// at 0b8fb240e6847c642f3bace52e52d54be5d414bc. Timestamp fixtures use one UTC
// representation; arbitrary legacy timestamp text is not chronologically sorted.
fn thread_messages(db: &mut Database) {
    db.query_sql(
        "CREATE TABLE content_documents (
        content_doc_id TEXT PRIMARY KEY, owner_kind TEXT NOT NULL, owner_id TEXT NOT NULL,
        space_id TEXT NOT NULL DEFAULT 'default', media_type TEXT NOT NULL, blob_path TEXT,
        sha256 TEXT NOT NULL DEFAULT '', size_bytes BIGINT NOT NULL DEFAULT 0,
        item_count BIGINT NOT NULL DEFAULT 0, schema_version BIGINT NOT NULL,
        created_at TEXT NOT NULL, updated_at TEXT NOT NULL, UNIQUE (owner_kind, owner_id))",
    )
    .unwrap();
    db.query_sql("CREATE TABLE thread_messages (
        content_message_id TEXT PRIMARY KEY, message_id TEXT NOT NULL,
        thread_storage_id TEXT NOT NULL, thread_id TEXT NOT NULL,
        content_doc_id TEXT NOT NULL REFERENCES content_documents(content_doc_id),
        space_id TEXT NOT NULL DEFAULT 'default', order_index BIGINT NOT NULL,
        role TEXT NOT NULL, content TEXT NOT NULL, timestamp TEXT,
        token_count BIGINT NOT NULL DEFAULT 0, metadata_json TEXT NOT NULL DEFAULT '{}',
        external_id TEXT NOT NULL DEFAULT '', exclude_from_distillation BOOLEAN NOT NULL DEFAULT FALSE,
        content_hash TEXT NOT NULL, created_at TEXT NOT NULL, updated_at TEXT NOT NULL)").unwrap();
    for statement in [
        "CREATE INDEX idx_thread_messages_order ON thread_messages (thread_storage_id, order_index, content_message_id)",
        "CREATE INDEX idx_thread_messages_thread ON thread_messages (thread_storage_id)",
        "CREATE INDEX idx_thread_messages_space ON thread_messages (space_id, thread_storage_id)",
        "CREATE INDEX idx_thread_messages_message_id ON thread_messages (message_id)",
        "INSERT INTO content_documents (content_doc_id, owner_kind, owner_id, media_type, schema_version, created_at, updated_at) VALUES ('doc', 'thread', 'thread', 'application/json', 1, '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z')",
    ] {
        db.query_sql(statement).unwrap();
    }
    for (id, space, thread, timestamp) in [
        (
            "m1",
            "s1",
            "thread",
            Value::String("2026-10-01T01:00:00Z".into()),
        ),
        (
            "m2",
            "s1",
            "thread",
            Value::String("2026-10-01T01:00:00Z".into()),
        ),
        (
            "m3",
            "s1",
            "thread",
            Value::String("2026-10-01T03:00:00Z".into()),
        ),
        ("m4", "s1", "thread", Value::Null),
        (
            "m5",
            "s2",
            "thread",
            Value::String("2026-10-01T02:00:00Z".into()),
        ),
    ] {
        db.query_sql_with_params("INSERT INTO thread_messages (content_message_id, message_id, thread_storage_id, thread_id, content_doc_id, space_id, order_index, role, content, timestamp, content_hash, created_at, updated_at) VALUES ($1, $1, $3, $3, 'doc', $2, 0, 'user', 'hello', $4, 'hash', '2026-10-02T00:00:00Z', '2026-10-02T00:00:00Z')", &[Value::String(id.into()), Value::String(space.into()), Value::String(thread.into()), timestamp]).unwrap();
    }
    // Make a selective point read cheaper than scanning a tiny two-row view.
    for ordinal in 0..32 {
        let id = Value::String(format!("other-{ordinal}"));
        db.query_sql_with_params("INSERT INTO thread_messages (content_message_id, message_id, thread_storage_id, thread_id, content_doc_id, space_id, order_index, role, content, timestamp, content_hash, created_at, updated_at) VALUES ($1, $1, $1, $1, 'doc', 's2', 0, 'user', 'hello', NULL, 'hash', '2026-10-02T00:00:00Z', '2026-10-02T00:00:00Z')", &[id]).unwrap();
    }
}

const THREAD_SUMMARY: &str = "SELECT space_id, thread_storage_id, COUNT(*) AS message_count, COUNT(timestamp) AS timestamp_count, MIN(timestamp) AS first_message_at, MAX(timestamp) AS last_message_at FROM thread_messages GROUP BY space_id, thread_storage_id";

fn assert_threads_match(db: &mut Database) {
    let expected = db.query_sql(THREAD_SUMMARY).unwrap();
    let actual = db.query_sql("SELECT * FROM thread_activity").unwrap();
    let normalize = |output: QueryOutput| {
        let mut rows = output
            .rows
            .iter()
            .map(|row| format!("{row:?}"))
            .collect::<Vec<_>>();
        rows.sort();
        rows
    };
    assert_eq!(normalize(actual), normalize(expected));
}

#[test]
fn materialized_thread_activity_indexed_reads_recover_and_retract() {
    use hawdb_storage::config::RelationalIndexMode;
    for mode in [
        RelationalIndexMode::Shadow,
        RelationalIndexMode::DemandPaged,
    ] {
        let path = unique_test_dir("materialized_thread_activity");
        let config = DatabaseConfig {
            relational_index_mode: mode,
            ..DatabaseConfig::default()
        };
        {
            let mut db = Database::open_with_config(&path, config.clone()).unwrap();
            thread_messages(&mut db);
            db.query_sql(&format!(
                "CREATE MATERIALIZED VIEW thread_activity AS {THREAD_SUMMARY}"
            ))
            .unwrap();
            db.query_sql("CREATE INDEX activity_space_thread ON thread_activity (space_id, thread_storage_id)").unwrap();
            assert_threads_match(&mut db);
            let sql =
                "SELECT * FROM thread_activity WHERE space_id = $1 AND thread_storage_id = $2";
            let params = [Value::String("s1".into()), Value::String("thread".into())];
            let selected = db
                .query_sql_with_params_options(
                    sql,
                    &params,
                    QueryStreamOptions {
                        max_rows: Some(1),
                        max_payload_bytes: Some(2048),
                    },
                )
                .unwrap();
            assert_eq!(selected.rows[0]["message_count"], Value::Int(4));
            assert_eq!(selected.rows[0]["timestamp_count"], Value::Int(3));
            let explain = db
                .query_sql_with_params(&format!("EXPLAIN ANALYZE {sql}"), &params)
                .unwrap();
            let access = explain.rows.iter().find(|row| {
                matches!(row.get("access object"), Some(Value::String(object)) if object.contains("activity_space_thread"))
            }).unwrap_or_else(|| panic!("missing activity index access: {:?}", explain.rows));
            assert_eq!(access.get("actRows"), Some(&Value::Int(1)));
            db.query_sql("DELETE FROM thread_messages WHERE content_message_id = 'm1'")
                .unwrap();
            assert_threads_match(&mut db);
        }
        // First reopen replays creation and a duplicate-minimum deletion from WAL.
        {
            let mut db = Database::open_with_config(&path, config.clone()).unwrap();
            assert_threads_match(&mut db);
            db.query_sql("DELETE FROM thread_messages WHERE content_message_id = 'm2'")
                .unwrap();
            assert_threads_match(&mut db);
            let before = db.query_sql("SELECT * FROM thread_activity").unwrap().rows;
            {
                let mut tx = db.begin_transaction().unwrap();
                tx.query_sql("UPDATE thread_messages SET timestamp = '2026-10-01T04:00:00Z' WHERE content_message_id = 'm3'").unwrap();
                assert_eq!(
                    tx.query_sql(
                        "SELECT last_message_at FROM thread_activity WHERE space_id = 's1'"
                    )
                    .unwrap()
                    .rows[0]["last_message_at"],
                    Value::String("2026-10-01T04:00:00Z".into())
                );
            }
            assert_eq!(
                db.query_sql("SELECT * FROM thread_activity").unwrap().rows,
                before
            );
            db.checkpoint().unwrap();
            db.query_sql("UPDATE thread_messages SET space_id = 's2', timestamp = '2026-10-01T05:00:00Z' WHERE content_message_id = 'm3'").unwrap();
        }
        {
            let mut db = Database::open_with_config(&path, config).unwrap();
            assert_threads_match(&mut db);
            let null_group = db
                .query_sql("SELECT * FROM thread_activity WHERE space_id = 's1'")
                .unwrap();
            assert_eq!(null_group.rows[0]["first_message_at"], Value::Null);
            assert_eq!(null_group.rows[0]["timestamp_count"], Value::Int(0));
            db.query_sql("DELETE FROM thread_messages WHERE space_id = 's2'")
                .unwrap();
            assert_threads_match(&mut db);
            db.query_sql("DELETE FROM thread_messages WHERE space_id = 's1'")
                .unwrap();
            assert!(db
                .query_sql("SELECT * FROM thread_activity")
                .unwrap()
                .rows
                .is_empty());
        }
        std::fs::remove_dir_all(path).unwrap();
    }
}

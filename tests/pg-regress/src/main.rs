// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};

use hawdb::{
    ConcurrentDatabase, ConcurrentDatabaseTransaction, ConcurrentTransactionOptions, Database,
    HawDBError, SqlStatementResult, TransactionIsolation, Value,
};
use serde::Deserialize;
use serde_json::{json, Value as Json};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Reset,
    Begin {
        session: String,
        isolation: Isolation,
    },
    Execute {
        session: String,
        sql: String,
    },
    Commit {
        session: String,
    },
    Rollback {
        session: String,
    },
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Isolation {
    RepeatableRead,
    Serializable,
}

struct Session {
    isolation: Isolation,
    transaction: Option<ConcurrentDatabaseTransaction>,
}

struct Worker {
    database: ConcurrentDatabase,
    sessions: BTreeMap<String, Session>,
}

fn cell(value: &Value) -> Json {
    match value {
        Value::Null => json!({"kind": "null"}),
        Value::Bool(value) => json!({"kind": "bool", "value": value}),
        Value::Int(value) => json!({"kind": "int", "value": value.to_string()}),
        Value::Float(value) => json!({"kind": "float", "value": value.to_string()}),
        Value::String(value) => json!({"kind": "text", "value": value}),
        value => json!({"kind": "unmapped", "value": format!("{value:?}")}),
    }
}

fn output(result: SqlStatementResult) -> Json {
    let columns = result.output.schema().columns();
    let rows = result
        .output
        .value_rows()
        .map(|row| row.iter().map(cell).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    json!({"status": "ok", "columns": columns, "rows": rows,
        "affected": result.mutation.map_or(0, |mutation| mutation.affected_rows)})
}

impl Worker {
    fn new() -> Self {
        Self {
            database: Database::new().into_concurrent(),
            sessions: BTreeMap::new(),
        }
    }

    fn execute(&mut self, request: Request) -> hawdb::Result<Json> {
        match request {
            Request::Reset => {
                *self = Self::new();
            }
            Request::Begin { session, isolation } => {
                if self.sessions.contains_key(&session) {
                    return Err(HawDBError::Semantic(
                        "session already has a transaction".into(),
                    ));
                }
                self.sessions.insert(
                    session,
                    Session {
                        isolation,
                        transaction: None,
                    },
                );
            }
            Request::Execute { session, sql } => {
                if let Some(session) = self.sessions.get_mut(&session) {
                    // PostgreSQL establishes REPEATABLE READ/Serializable snapshots at
                    // the first statement, not at BEGIN. Match that capture boundary.
                    if session.transaction.is_none() {
                        let isolation = match session.isolation {
                            Isolation::RepeatableRead => TransactionIsolation::SnapshotIsolation,
                            Isolation::Serializable => TransactionIsolation::Serializable,
                        };
                        session.transaction = Some(self.database.begin_transaction(
                            ConcurrentTransactionOptions::optimistic().with_isolation(isolation),
                        )?);
                    }
                    let transaction = session.transaction.as_mut().ok_or_else(|| {
                        HawDBError::Semantic("missing transaction after begin".into())
                    })?;
                    return transaction.query_sql_with_result(&sql).map(output);
                }
                let mut transaction = self
                    .database
                    .begin_transaction(ConcurrentTransactionOptions::optimistic())?;
                let result = transaction.query_sql_with_result(&sql)?;
                transaction.commit()?;
                return Ok(output(result));
            }
            Request::Commit { session } => {
                let session = self
                    .sessions
                    .remove(&session)
                    .ok_or_else(|| HawDBError::Semantic("commit without begin".into()))?;
                if let Some(transaction) = session.transaction {
                    transaction.commit()?;
                }
            }
            Request::Rollback { session } => {
                let session = self
                    .sessions
                    .remove(&session)
                    .ok_or_else(|| HawDBError::Semantic("rollback without begin".into()))?;
                if let Some(transaction) = session.transaction {
                    transaction.rollback();
                }
            }
        }
        Ok(json!({"status": "ok", "columns": [], "rows": [], "affected": 0}))
    }
}

fn error(error: HawDBError) -> Json {
    let kind = match &error {
        HawDBError::TransactionConflict { .. } => "serialization_failure",
        HawDBError::Parse(_) => "parse",
        HawDBError::Semantic(_) => "semantic",
        HawDBError::TransactionReadDependencyRefused { .. } => "read_dependency_refused",
        _ => "execution",
    };
    json!({"status": "error", "kind": kind, "message": error.to_string()})
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut worker = Worker::new();
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.len() > 1_048_576 {
            return Err("request exceeds 1 MiB".into());
        }
        let request: Request = serde_json::from_str(&line)?;
        let response = worker.execute(request).unwrap_or_else(error);
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}

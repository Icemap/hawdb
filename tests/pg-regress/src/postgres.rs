// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use libpq::{result::ErrorField, Connection, Status};
use serde_json::{json, Value as Json};

type Result<T> = std::result::Result<T, String>;

fn cell(oid: u32, value: Option<&[u8]>) -> Result<Json> {
    let Some(value) = value else {
        return Ok(json!({"kind": "null"}));
    };
    if value.len() > 1_048_576 {
        return Err("reference cell exceeds 1 MiB fixture limit".into());
    }
    let value = std::str::from_utf8(value).map_err(|_| "invalid UTF-8 reference value")?;
    Ok(match oid {
        16 => json!({"kind": "bool", "value": match value {
            "t" => true, "f" => false, _ => return Err("invalid reference Boolean".into()),
        }}),
        20 | 21 | 23 => json!({"kind": "int", "value": value.parse::<i64>()
            .map_err(|_| "invalid reference integer")?.to_string()}),
        700 | 701 => json!({"kind": "float", "value": value}),
        18 | 19 | 25 | 1042 | 1043 => json!({"kind": "text", "value": value}),
        _ => json!({"kind": format!("pg_oid_{oid}"), "value": value}),
    })
}

pub(crate) fn execute(connection: &Connection, sql: &str) -> Result<Json> {
    if sql.contains('\0') {
        return Err("SQL contains a NUL byte".into());
    }
    let result = connection.exec(sql);
    if !matches!(result.status(), Status::CommandOk | Status::TuplesOk) {
        if connection.status() != libpq::connection::Status::Ok {
            return Err("PostgreSQL connection lost".into());
        }
        let code = result
            .error_field(ErrorField::Sqlstate)
            .map_err(|_| "invalid SQLSTATE")?;
        if matches!(code, Some("57014" | "55P03")) {
            return Err(
                "reference statement/lock timeout; wait-aware schedules are unsupported".into(),
            );
        }
        let message = result
            .error_field(ErrorField::MessagePrimary)
            .map_err(|_| "invalid reference error")?;
        return Ok(json!({"status": "error", "sqlstate": code, "message": message}));
    }
    if result.ntuples() > 4096 {
        return Err("reference result exceeds 4096-row fixture limit".into());
    }
    let columns = (0..result.nfields())
        .map(|column| {
            result
                .field_name(column)
                .map_err(|_| "invalid reference column name".to_string())?
                .ok_or_else(|| "missing reference column name".to_string())
        })
        .collect::<Result<Vec<_>>>()?;
    let types = (0..result.nfields())
        .map(|column| result.field_type(column))
        .collect::<Vec<_>>();
    let rows = (0..result.ntuples())
        .map(|row| {
            types
                .iter()
                .enumerate()
                .map(|(column, oid)| cell(*oid, result.value(row, column)))
                .collect::<Result<Vec<_>>>()
        })
        .collect::<Result<Vec<_>>>()?;
    let tag = result
        .cmd_status()
        .map_err(|_| "invalid reference command tag")?
        .unwrap_or_default();
    let affected = if matches!(
        tag.split_whitespace().next(),
        Some("INSERT" | "UPDATE" | "DELETE")
    ) {
        result
            .cmd_tuples()
            .map_err(|_| "invalid affected-row count")?
    } else {
        0
    };
    Ok(json!({"status": "ok", "columns": columns, "rows": rows,
        "pg_types": types, "affected": affected, "command_tag": tag}))
}

pub(crate) fn connect(dsn_env: &str) -> Result<Connection> {
    let dsn = std::env::var(dsn_env)
        .map_err(|_| format!("set {dsn_env} to a PostgreSQL 18.0 conninfo"))?;
    // Parse URI/keyword conninfo through libpq rather than quoting credentials.
    // Override the timeout after parsing; no DSN or library debug error is logged.
    let mut options = libpq::connection::Info::from(&dsn)
        .map_err(|_| "invalid reference connection options")?
        .into_iter()
        .filter_map(|info| info.val.map(|value| (info.keyword, value)))
        .collect::<HashMap<_, _>>();
    options.insert("connect_timeout".into(), "5".into());
    let borrowed = options
        .iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect();
    let connection = Connection::with_params(&borrowed, false)
        .map_err(|_| "PostgreSQL connection failed; check the DSN environment variable")?;
    let version = connection.server_version();
    if version != 180000 {
        return Err(format!("expected PostgreSQL 18.0 (180000), got {version}"));
    }
    for sql in [
        "SET client_encoding = 'UTF8'",
        "SET timezone = 'UTC'",
        "SET datestyle = 'ISO, YMD'",
        "SET extra_float_digits = 3",
        "SET statement_timeout = '3000ms'",
        "SET lock_timeout = '500ms'",
        "SET client_min_messages = warning",
    ] {
        if execute(&connection, sql)?["status"] != "ok" {
            return Err("failed to initialize reference session".into());
        }
    }
    Ok(connection)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_value_decoder_keeps_kinds_and_integer_precision() {
        assert_eq!(
            cell(20, Some(b"9223372036854775807")).unwrap()["value"],
            "9223372036854775807"
        );
        assert_ne!(cell(16, None).unwrap(), cell(25, Some(b"")).unwrap());
        assert_ne!(cell(16, Some(b"f")).unwrap(), cell(20, Some(b"0")).unwrap());
        assert_ne!(cell(20, Some(b"0")).unwrap(), cell(25, Some(b"0")).unwrap());
        assert_eq!(cell(21, Some(b"1")).unwrap(), cell(23, Some(b"1")).unwrap());
    }

    #[test]
    fn invalid_values_and_unmapped_types_are_not_coerced() {
        assert!(cell(16, Some(b"false")).is_err());
        assert!(cell(20, Some(b"9223372036854775808")).is_err());
        assert_eq!(cell(1700, Some(b"1.1")).unwrap()["kind"], "pg_oid_1700");
    }
}

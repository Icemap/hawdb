// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::embedded::{Embedded, Isolation, Request};
use crate::suite::{Case, Result, Suite};
use crate::{compare, postgres, suite};

pub(crate) struct Options {
    pub dsn_env: String,
    pub filter: String,
    pub schedule: PathBuf,
    pub report: PathBuf,
    pub list: bool,
}

pub(crate) fn project() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

impl Options {
    pub(crate) fn parse() -> Result<Option<Self>> {
        let mut options = Self {
            dsn_env: "PGREGRESS_DSN".into(),
            filter: String::new(),
            schedule: project().join("data/schedule"),
            report: project().join("../../target/pg-regress/report.json"),
            list: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--help" | "-h" => {
                    println!("hawdb-pg-regress [--list] [--case SUBSTRING] [--schedule PATH] [--report PATH] [--dsn-env NAME]\nReference: PostgreSQL 18.0, credentials from PGREGRESS_DSN. Manual only.");
                    return Ok(None);
                }
                "--list" => options.list = true,
                "--case" => options.filter = args.next().ok_or("--case needs a value")?,
                "--dsn-env" => options.dsn_env = args.next().ok_or("--dsn-env needs a value")?,
                "--schedule" => {
                    options.schedule = args.next().ok_or("--schedule needs a value")?.into()
                }
                "--report" => options.report = args.next().ok_or("--report needs a value")?.into(),
                _ => return Err(format!("unknown argument {flag}")),
            }
        }
        Ok(Some(options))
    }
}

fn git(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(project().join("../.."))
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("could not read source revision".into());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn stamp() -> Result<u128> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_nanos())
        .map_err(|e| e.to_string())
}

fn run_case(case: &Case, options: &Options) -> Result<Value> {
    if let Some(reason) = &case.unsupported {
        return Ok(json!({"id":case.id,"status":"UNSUPPORTED","reason":reason,"trace":[]}));
    }
    let namespace = format!("hawdb_pg18_{}_{}", std::process::id(), stamp()?);
    let admin = postgres::connect(&options.dsn_env)?;
    if postgres::execute(&admin, &format!("CREATE SCHEMA \"{namespace}\""))?["status"] != "ok" {
        return Err("could not create isolated test schema".into());
    }
    let mut sessions = BTreeMap::new();
    let mut database = Embedded::new();
    let mut result = json!({"id":case.id,"status":"PASS","trace":[]});
    let execution = (|| -> Result<()> {
        for step in &case.steps {
            let session = step.session.as_deref().unwrap_or("main");
            if !sessions.contains_key(session) {
                let connection = postgres::connect(&options.dsn_env)?;
                if postgres::execute(&connection, &format!("SET search_path TO \"{namespace}\""))?
                    ["status"]
                    != "ok"
                {
                    return Err("could not select test schema".into());
                }
                sessions.insert(session.to_owned(), connection);
            }
            let sql = step.execute_sql.as_deref().unwrap_or(&step.sql);
            let pg = postgres::execute(
                sessions
                    .get(session)
                    .ok_or("reference session disappeared")?,
                sql,
            )?;
            let request = match step.action.as_str() {
                "begin" => Request::Begin {
                    session: session.into(),
                    isolation: match step.isolation.as_deref() {
                        Some("repeatable_read") => Isolation::RepeatableRead,
                        Some("serializable") => Isolation::Serializable,
                        _ => return Err("unsupported isolation".into()),
                    },
                },
                "commit" => Request::Commit {
                    session: session.into(),
                },
                "rollback" => Request::Rollback {
                    session: session.into(),
                },
                "execute" => Request::Execute {
                    session: session.into(),
                    sql: sql.into(),
                },
                _ => return Err("unsupported action".into()),
            };
            let hawdb = database.request(request);
            let matches = compare::equivalent(&pg, &hawdb, step)?;
            result["trace"]
                .as_array_mut()
                .ok_or("missing trace")?
                .push(json!({"step":step,"postgres":pg,"hawdb":hawdb}));
            if !matches {
                result["status"] = json!("MISMATCH");
                result["reason"] = json!("first differing statement; remaining steps not executed");
                break;
            }
        }
        Ok(())
    })();
    // Closing every connection rolls back unfinished transactions before DROP.
    drop(sessions);
    let cleanup = postgres::execute(&admin, &format!("DROP SCHEMA \"{namespace}\" CASCADE"));
    if !cleanup
        .as_ref()
        .is_ok_and(|response| response["status"] == "ok")
    {
        return Err(format!(
            "cleanup failed; retained test schema {namespace}; prior failure: {}",
            execution.as_ref().err().map_or("none", String::as_str)
        ));
    }
    execution?;
    Ok(result)
}

fn counts(report: &Value) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(cases) = report["cases"].as_array() {
        for case in cases {
            if let Some(status) = case["status"].as_str() {
                *counts.entry(status.to_owned()).or_default() += 1;
            }
        }
    }
    counts
}

fn save(path: &Path, report: &Value) -> Result<()> {
    let parent = path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut bytes = serde_json::to_vec_pretty(report).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|e| e.to_string())
}

fn execute(options: &Options, report: &mut Value) -> Result<()> {
    let suite = Suite::load(project())?;
    let cases = suite.select(&options.schedule, &options.filter)?;
    report["sources"] = serde_json::to_value(&suite.sources).map_err(|e| e.to_string())?;
    report["selected_cases"] = json!(cases.len());
    report["hawdb_revision"] = json!(git(&["rev-parse", "HEAD"])?.trim());
    report["working_tree_dirty"] = json!(!git(&["status", "--porcelain"])?.is_empty());
    let mut hashes = BTreeMap::new();
    for name in [
        "src/main.rs",
        "src/regress.rs",
        "src/suite.rs",
        "src/compare.rs",
        "src/embedded.rs",
        "src/postgres.rs",
        "Cargo.toml",
        "Cargo.lock",
        "data/cases.json",
    ] {
        hashes.insert(name.to_owned(), suite::digest(&project().join(name))?);
    }
    hashes.insert(
        "selected_schedule".into(),
        suite::digest(&options.schedule)?,
    );
    report["harness_sha256"] = json!(hashes);
    report["executable_sha256"] = json!(suite::digest(
        &std::env::current_exe().map_err(|e| e.to_string())?
    )?);
    // Gate the real server before creating any schema or embedded database.
    drop(postgres::connect(&options.dsn_env)?);
    save(&options.report, report)?;
    for case in cases {
        report["active_case"] = json!(case.id);
        let observation = run_case(case, options)?;
        let trace = observation["trace"].as_array().ok_or("missing trace")?;
        println!(
            "{:<11} {} ({} statements)",
            observation["status"].as_str().unwrap_or("INFRA_ERROR"),
            case.id,
            trace.len()
        );
        match observation["status"].as_str() {
            Some("UNSUPPORTED") => println!(
                "  Reason: {}",
                observation["reason"].as_str().unwrap_or("unknown")
            ),
            Some("MISMATCH") => {
                let last = trace.last().ok_or("missing differing statement")?;
                println!(
                    "  SQL: {}\n  PG: {}\n  HawDB: {}",
                    last["step"]["sql"].as_str().unwrap_or(""),
                    last["postgres"],
                    last["hawdb"]
                );
            }
            _ => {}
        }
        report["cases"]
            .as_array_mut()
            .ok_or("missing cases")?
            .push(observation);
        report["counts"] = json!(counts(report));
        save(&options.report, report)?;
    }
    report
        .as_object_mut()
        .ok_or("invalid report")?
        .remove("active_case");
    report["completed"] = json!(true);
    Ok(())
}

pub(crate) fn run(options: Options) -> i32 {
    if options.list {
        return match Suite::load(project()).and_then(|suite| {
            for case in suite.select(&options.schedule, &options.filter)? {
                println!(
                    "{:<11} {}",
                    if case.unsupported.is_some() {
                        "UNSUPPORTED"
                    } else {
                        "READY"
                    },
                    case.id
                );
            }
            Ok(())
        }) {
            Ok(()) => 0,
            Err(error) => {
                eprintln!("INFRA_ERROR: {error}");
                2
            }
        };
    }
    let mut report = json!({"postgres_version":180000,"postgres_commit":suite::UPSTREAM,"cases":[],"selection":options.filter,"completed":false});
    if let Err(error) = save(&options.report, &report) {
        eprintln!("INFRA_ERROR: could not initialize report: {error}");
        return 2;
    }
    let execution = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute(&options, &mut report)
    }));
    let error = match execution {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(error),
        Err(_) => Some("comparison panicked; run incomplete".into()),
    };
    if let Some(error) = error {
        eprintln!("INFRA_ERROR: {error}");
        report["infrastructure_error"] = json!(error);
    }
    let counts = counts(&report);
    report["counts"] = json!(counts);
    if let Err(error) = save(&options.report, &report) {
        eprintln!("INFRA_ERROR: could not save report: {error}");
        return 2;
    }
    println!(
        "SUMMARY {} | report: {}",
        report["counts"],
        options.report.display()
    );
    if report.get("infrastructure_error").is_some() {
        2
    } else if counts.get("MISMATCH").copied().unwrap_or(0) > 0
        || counts.get("PASS").copied().unwrap_or(0) == 0
    {
        1
    } else {
        0
    }
}

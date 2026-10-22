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
use hawdb_storage::file_descriptors::ProjectFileDescriptors;

#[test]
fn default_soft_descriptor_capacity_opens_commits_checkpoints_and_reopens() {
    assert!(!DatabaseConfig::default().strict_capacity_limit);
    assert!(
        !DatabaseConfig::default()
            .wal_replay_config()
            .strict_capacity_limit
    );
    let path = unique_test_dir("soft_descriptor_capacity");
    let config = DatabaseConfig {
        max_open_files: 2,
        storage_residency_mode: crate::StorageResidencyMode::Materialized,
        ..DatabaseConfig::default()
    };
    {
        let mut db = Database::open_with_config(&path, config.clone()).unwrap();
        db.query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY, value BIGINT)")
            .unwrap();
        db.query_sql("INSERT INTO records (id, value) VALUES (1, 10)")
            .unwrap();
        db.checkpoint().unwrap();
        let metrics = db.file_descriptor_metrics().unwrap();
        assert!(!metrics.strict_capacity_limit);
        assert!(metrics.high_water > metrics.limit);
        assert_eq!(metrics.budget_rejections, 0);
    }
    let db = Database::open_with_config(&path, config).unwrap();
    let rows = db
        .begin_read_transaction()
        .unwrap()
        .query_sql("SELECT value FROM records WHERE id = 1")
        .unwrap();
    assert_eq!(rows.rows[0].get("value"), Some(&Value::Int(10)));
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn strict_foreground_capacity_does_not_block_wal_or_checkpoint_completion() {
    let path = unique_test_dir("strict_descriptor_maintenance");
    let config = DatabaseConfig {
        max_open_files: 32,
        strict_capacity_limit: true,
        storage_residency_mode: crate::StorageResidencyMode::Materialized,
        ..DatabaseConfig::default()
    };
    {
        let mut db = Database::open_with_config(&path, config.clone()).unwrap();
        db.query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY, value BIGINT)")
            .unwrap();
        db.query_sql("INSERT INTO records (id, value) VALUES (1, 10)")
            .unwrap();
        db.checkpoint().unwrap();
        let context = db.runtime.file_descriptor_context().unwrap();
        let project = ProjectFileDescriptors::from_open_context(&context);
        let before = project.metrics();
        let held = project
            .reserve(
                before.limit.saturating_sub(before.open + before.reserved) + before.cached_handles,
            )
            .unwrap();
        assert!(project.reserve(1).is_err());
        let rejected = project.metrics().budget_rejections;
        db.query_sql("INSERT INTO records (id, value) VALUES (2, 20)")
            .unwrap();
        db.checkpoint().unwrap();
        assert_eq!(project.metrics().budget_rejections, rejected);
        assert!(project.metrics().high_water > project.metrics().limit);
        assert_eq!(project.metrics().open, before.open - before.cached_handles);
        drop(held);
        assert_eq!(project.metrics().reserved, 0);
        assert_eq!(project.metrics().open, before.open - before.cached_handles);
    }
    let db = Database::open_with_config(&path, config).unwrap();
    let rows = db
        .begin_read_transaction()
        .unwrap()
        .query_sql("SELECT SUM(value) AS total FROM records")
        .unwrap();
    assert_eq!(rows.rows[0].get("total"), Some(&Value::Int(30)));
    drop(db);
    std::fs::remove_dir_all(path).unwrap();
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[test]
fn os_headroom_refuses_before_wal_effect_and_keeps_the_database_retryable() {
    const CHILD: &str = "HAWDB_FD_OS_HEADROOM_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                concat!(
                    module_path!(),
                    "::os_headroom_refuses_before_wal_effect_and_keeps_the_database_retryable"
                )
                .strip_prefix("hawdb::")
                .unwrap(),
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"));
        return;
    }
    let mut limits = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: The pointer is writable and correctly sized; this is an isolated child.
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut limits) },
        0
    );
    limits.rlim_cur = 256;
    // SAFETY: Only the expendable child lowers its own soft limit.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &limits) }, 0);
    let path = unique_test_dir("os_headroom_wal_retry");
    let config = DatabaseConfig {
        max_open_files: 32,
        storage_residency_mode: crate::StorageResidencyMode::Materialized,
        ..Default::default()
    };
    let mut db = Database::open_with_config(&path, config.clone()).unwrap();
    db.query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY, value BIGINT)")
        .unwrap();
    db.query_sql("INSERT INTO records (id, value) VALUES (1, 10)")
        .unwrap();
    drop(db);
    let mut db = Database::open_with_config(&path, config.clone()).unwrap();
    let rows = db
        .begin_read_transaction()
        .unwrap()
        .query_sql("SELECT value FROM records WHERE id = 1")
        .unwrap();
    assert_eq!(rows.rows[0].get("value"), Some(&Value::Int(10)));
    let context = db.runtime.file_descriptor_context().unwrap();
    let project = ProjectFileDescriptors::from_open_context(&context);
    let before = project.metrics();
    let allowance = 256 - 64;
    let hawdb_storage::branch_project::ProjectManifest::Branch(selector) =
        hawdb_storage::branch_project::inspect_project_manifest(&path).unwrap()
    else {
        panic!("fixture must publish a branch project")
    };
    let branch = path
        .join("branches")
        .join(selector.main_branch_id().as_uuid().to_string());
    let wal_bytes = std::fs::read_dir(branch)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("wal.")
        })
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    assert!(!wal_bytes.is_empty());
    let competing_project = project.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let competitor = std::thread::spawn(move || {
        let _held = competing_project
            .reserve(allowance - before.open - before.reserved + before.cached_handles)
            .unwrap();
        ready_tx.send(()).unwrap();
        let _ = release_rx.recv();
    });
    ready_rx.recv().unwrap();
    assert_eq!(
        project.metrics().open + project.metrics().reserved,
        allowance
    );
    let error = db
        .query_sql("INSERT INTO records (id, value) VALUES (2, 20)")
        .unwrap_err();
    assert!(
        matches!(
            hawdb_core::error::file_descriptor_error(&error),
            Some(hawdb_core::error::FileDescriptorError::OsLimit {
                os_code: None,
                soft: Some(256),
                ..
            })
        ),
        "{error:?}"
    );
    for (path, bytes) in wal_bytes {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    release_tx.send(()).unwrap();
    competitor.join().unwrap();
    assert_eq!(project.metrics().reserved, before.reserved);
    assert!(project.metrics().open <= before.open);
    db.query_sql("INSERT INTO records (id, value) VALUES (2, 20)")
        .unwrap();
    let rows = db
        .begin_read_transaction()
        .unwrap()
        .query_sql("SELECT SUM(value) AS total FROM records")
        .unwrap();
    assert_eq!(rows.rows[0].get("total"), Some(&Value::Int(30)));
    drop(db);
    let reopened = Database::open_with_config(&path, config).unwrap();
    let rows = reopened
        .begin_read_transaction()
        .unwrap()
        .query_sql("SELECT SUM(value) AS total FROM records")
        .unwrap();
    assert_eq!(rows.rows[0].get("total"), Some(&Value::Int(30)));
    drop(reopened);
    std::fs::remove_dir_all(path).unwrap();
}

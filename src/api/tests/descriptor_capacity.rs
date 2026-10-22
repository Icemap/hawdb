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
        drop(held);
        assert_eq!(project.metrics().reserved, 0);
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

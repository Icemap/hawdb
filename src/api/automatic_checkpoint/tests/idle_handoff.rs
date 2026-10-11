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
use crate::Value;
use std::collections::BTreeMap;

fn wait(control: &Control, predicate: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut state = control.lock().unwrap();
    while !predicate(&state) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "idle handoff stalled: {:?}, {:?}",
            state.phase,
            state.report
        );
        state = control.changed.wait_timeout(state, remaining).unwrap().0;
    }
}

fn database(root: &PathBuf) -> Database {
    Database::open_with_config(
        root,
        DatabaseConfig {
            automatic_checkpoint_max_age: Duration::from_millis(20),
            ..DatabaseConfig::default()
        },
    )
    .unwrap()
}

#[test]
fn idle_handle_adopts_and_retires_two_generations_without_frontend_access() {
    let fixture = Fixture::new();
    let mut db = database(&fixture.0);
    let resources = governor(16 * 1024 * 1024);
    db.set_runtime_governor(resources.clone());
    db.commit_epoch().unwrap();
    let control = db.runtime.checkpoint_control_for_test();
    for generation in 1..=2 {
        let before = control.report().unwrap().unwrap().completed_checkpoints;
        let mut transaction = db.begin_transaction().unwrap();
        for id in (generation - 1) * 16..generation * 16 {
            transaction
                .query_with_params(
                    "CREATE (:AutomaticIdle {id: $id})",
                    &BTreeMap::from([("id".into(), Value::Int(id))]),
                )
                .unwrap();
        }
        transaction.commit().unwrap();
        // Observe Control only: no facade get/read/write or manual checkpoint
        // can adopt the selected generation on the worker's behalf.
        wait(&control, |state| {
            state.phase == Phase::Idle
                && state.report.completed_checkpoints > before
                && !state.report.waiting_for_handoff
        });
        let report = resources.snapshot();
        assert_eq!(report.active_cpu_slots, 0);
        assert_eq!(report.active_background_tasks, 0);
        assert_eq!(report.active_background_io_slots, 0);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            control.report().unwrap().unwrap().completed_checkpoints,
            before + 1,
            "an adopted generation must replace the old captured source"
        );
    }
    let rows = db
        .query("MATCH (n:AutomaticIdle) RETURN n.id AS id ORDER BY id")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 32);
    for (id, row) in rows.iter().enumerate() {
        assert_eq!(row["id"], Value::Int(id as i64));
    }
    drop(db);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
    let mut reopened = database(&fixture.0);
    let rows = reopened
        .query("MATCH (n:AutomaticIdle) RETURN n.id AS id ORDER BY id")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 32);
    for (id, row) in rows.iter().enumerate() {
        assert_eq!(row["id"], Value::Int(id as i64));
    }
}

#[test]
fn releasing_a_runtime_read_guard_wakes_unattended_handoff() {
    let fixture = Fixture::new();
    let mut db = database(&fixture.0);
    let control = db.runtime.checkpoint_control_for_test();
    let suspension = db.runtime.suspend_automatic_checkpoint().unwrap();
    db.query("CREATE (:AutomaticIdle {id: 1})").unwrap();
    let runtime = db.runtime.get().unwrap();
    let before = runtime.store.checkpoint_source_identity();
    drop(suspension);
    wait(&control, |state| state.phase == Phase::Handoff);
    assert_eq!(runtime.store.checkpoint_source_identity(), before);
    assert!(control.report().unwrap().unwrap().waiting_for_handoff);
    drop(runtime);
    wait(&control, |state| {
        state.phase == Phase::Idle && !state.report.waiting_for_handoff
    });
    assert_eq!(
        db.query("MATCH (n:AutomaticIdle) RETURN n.id AS id")
            .unwrap()
            .rows[0]["id"],
        Value::Int(1)
    );
}

#[test]
fn releasing_runtime_read_guard_does_not_wait_for_control() {
    let fixture = Fixture::new();
    let db = database(&fixture.0);
    db.commit_epoch().unwrap();
    let control = db.runtime.checkpoint_control_for_test();
    let (ready, acquired) = std::sync::mpsc::channel();
    let (release, proceed) = std::sync::mpsc::channel();
    let (done, released) = std::sync::mpsc::channel();
    let reader = std::thread::spawn(move || {
        let runtime = db.runtime.get().unwrap();
        ready.send(()).unwrap();
        proceed.recv().unwrap();
        drop(runtime);
        done.send(()).unwrap();
        db
    });
    acquired.recv_timeout(Duration::from_secs(15)).unwrap();
    let state = control.lock().unwrap();
    release.send(()).unwrap();
    let observed = released.recv_timeout(Duration::from_secs(15));
    // Release Control even on failure, so joining the reader and closing its
    // database cannot mask the regression with a permanently blocked test.
    drop(state);
    drop(reader.join().unwrap());
    assert!(observed.is_ok(), "read guard release waited for Control");
}

#[test]
fn metadata_snapshots_outlive_background_adoption_and_the_handle() {
    use hawdb_storage::append_table::{AppendTableSchema, AppendTransaction, AppendWrite};
    use hawdb_storage::relational::{RelationalColumnSchema, RelationalScalarType};

    let fixture = Fixture::new();
    let mut db = Database::open_with_config(
        &fixture.0,
        DatabaseConfig {
            automatic_checkpoint_max_age: Duration::from_millis(20),
            relational_index_mode: hawdb_storage::config::RelationalIndexMode::Shadow,
            ..DatabaseConfig::default()
        },
    )
    .unwrap();
    let resources = governor(16 * 1024 * 1024);
    db.set_runtime_governor(resources.clone());
    let suspension = db.runtime.suspend_automatic_checkpoint().unwrap();
    db.append_transaction(AppendTransaction {
        writes: vec![AppendWrite::CreateTable {
            schema: AppendTableSchema {
                name: "events".into(),
                columns: vec![
                    RelationalColumnSchema {
                        name: "stream".into(),
                        scalar_type: RelationalScalarType::Text,
                        nullable: false,
                        default: None,
                    },
                    RelationalColumnSchema {
                        name: "sequence".into(),
                        scalar_type: RelationalScalarType::BigInt,
                        nullable: false,
                        default: None,
                    },
                ],
                partition_key: vec!["stream".into()],
                order_key: vec!["sequence".into()],
                order_mode: Default::default(),
            },
        }],
    })
    .unwrap();
    db.query_sql("CREATE TABLE records (id BIGINT PRIMARY KEY, body TEXT)")
        .unwrap();
    db.query_sql("INSERT INTO records (id, body) VALUES (1, 'before')")
        .unwrap();
    db.checkpoint().unwrap();
    let schema: AppendTableSchema = db.append_table_schema("events").unwrap().unwrap();
    let publication: crate::store::RelationalIndexShadowCheckpointReport = db
        .relational_index_shadow_checkpoint_report()
        .unwrap()
        .unwrap();
    let recovery_status: crate::store::RelationalIndexShadowRecoveryStatus =
        db.relational_index_shadow_recovery_status().unwrap();
    let recovery_report: Option<hawdb_storage::relational::RelationalIndexRecoveryReport> =
        db.relational_index_recovery_report().unwrap();
    let before = publication.source_commit_epoch;
    let snapshots = (schema, publication, recovery_status, recovery_report);
    let expected = snapshots.clone();
    let control = db.runtime.checkpoint_control_for_test();
    let mut transaction = db.begin_transaction().unwrap();
    transaction
        .query("CREATE (:AutomaticIdle {id: 1})")
        .unwrap();
    transaction
        .query_sql("INSERT INTO records (id, body) VALUES (2, 'after')")
        .unwrap();
    transaction.commit().unwrap();
    drop(suspension);
    wait(&control, |state| {
        state.phase == Phase::Idle
            && state.report.completed_checkpoints == 1
            && !state.report.waiting_for_handoff
    });
    let after = db
        .relational_index_shadow_checkpoint_report()
        .unwrap()
        .unwrap();
    assert!(after.source_commit_epoch > before);
    assert_eq!(
        db.append_table_schema("events").unwrap().unwrap(),
        snapshots.0
    );
    assert_eq!(snapshots, expected);
    drop(db);
    assert_eq!(snapshots, expected);
    assert_eq!(resources.snapshot().admitted_memory_bytes, 0);
    let mut reopened = database(&fixture.0);
    assert_eq!(
        reopened.append_table_schema("events").unwrap().unwrap(),
        snapshots.0
    );
    let rows = reopened
        .query_sql("SELECT id, body FROM records ORDER BY id")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["body"], Value::String("before".into()));
    assert_eq!(rows[1]["body"], Value::String("after".into()));
    assert_eq!(
        reopened
            .query("MATCH (n:AutomaticIdle) RETURN n.id AS id")
            .unwrap()
            .rows[0]["id"],
        Value::Int(1)
    );
}

#[test]
fn read_guard_release_before_park_keeps_the_handoff_notification() {
    let fixture = Fixture::new();
    let mut db = database(&fixture.0);
    let suspension = db.runtime.suspend_automatic_checkpoint().unwrap();
    db.query("CREATE (:AutomaticIdle {id: 1})").unwrap();
    let control = db.runtime.checkpoint_control_for_test();
    let (paused, observed) = std::sync::mpsc::channel();
    let (resume, continuation) = std::sync::mpsc::channel();
    control.lock().unwrap().handoff_park_probe = Some(Arc::new(OwnerPauseProbe {
        paused,
        resume: Mutex::new(continuation),
    }));
    let runtime = db.runtime.get().unwrap();
    drop(suspension);
    observed.recv_timeout(Duration::from_secs(15)).unwrap();
    // The worker has failed try_write and released Control, but has not yet
    // called park. No following frontend access or periodic wake is allowed.
    drop(runtime);
    resume.send(()).unwrap();
    wait(&control, |state| {
        state.phase == Phase::Idle && !state.report.waiting_for_handoff
    });
    assert_eq!(control.report().unwrap().unwrap().completed_checkpoints, 1);
    assert_eq!(
        db.query("MATCH (n:AutomaticIdle) RETURN n.id AS id")
            .unwrap()
            .rows[0]["id"],
        Value::Int(1)
    );
}

#[test]
fn stopping_a_parked_handoff_does_not_wait_for_the_runtime_read_guard() {
    let fixture = Fixture::new();
    let mut db = database(&fixture.0);
    let resources = governor(16 * 1024 * 1024);
    db.set_runtime_governor(resources.clone());
    let suspension = db.runtime.suspend_automatic_checkpoint().unwrap();
    db.query("CREATE (:AutomaticIdle {id: 1})").unwrap();
    let control = db.runtime.checkpoint_control_for_test();
    let shared = db.runtime.shared_runtime_for_test();
    let runtime = shared.read(&control).unwrap().unwrap();
    drop(suspension);
    wait(&control, |state| state.phase == Phase::Handoff);
    let (done, stopped) = std::sync::mpsc::channel();
    let closer = std::thread::spawn(move || {
        drop(db);
        done.send(()).unwrap();
    });
    let observed = stopped.recv_timeout(Duration::from_secs(15));
    // Permit cleanup even if shutdown incorrectly waited for the read lock.
    drop(runtime);
    drop(shared);
    closer.join().unwrap();
    assert!(
        observed.is_ok(),
        "shutdown waited for the runtime read guard"
    );
    let report = resources.snapshot();
    assert_eq!(report.active_cpu_slots, 0);
    assert_eq!(report.active_background_tasks, 0);
    assert_eq!(report.active_background_io_slots, 0);
    assert_eq!(report.admitted_memory_bytes, 0);
    let mut reopened = database(&fixture.0);
    assert_eq!(
        reopened
            .query("MATCH (n:AutomaticIdle) RETURN n.id AS id")
            .unwrap()
            .rows[0]["id"],
        Value::Int(1)
    );
}

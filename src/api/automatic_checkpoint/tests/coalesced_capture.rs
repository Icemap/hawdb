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
use std::sync::mpsc;

fn append(database: &mut Database, id: i64) {
    database
        .query_with_params(
            "CREATE (:CoalescedWrite {id: $id, body: $body})",
            &BTreeMap::from([
                ("id".into(), Value::Int(id)),
                ("body".into(), Value::String("coalesced:界🙂\0".repeat(16))),
            ]),
        )
        .unwrap();
}

fn wait(control: &Control, predicate: impl Fn(&State) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let state = control.lock().unwrap();
        if predicate(&state) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{:?}: {:?}",
            state.phase,
            state.report
        );
        drop(state);
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn verify(database: &mut Database, count: usize) {
    let rows = database
        .query("MATCH (n:CoalescedWrite) RETURN n.id AS id, n.body AS body ORDER BY id")
        .unwrap()
        .rows;
    assert_eq!(rows.len(), count);
    for (id, row) in rows.iter().enumerate() {
        assert_eq!(row["id"], Value::Int(id as i64));
        assert_eq!(row["body"], Value::String("coalesced:界🙂\0".repeat(16)));
    }
}

#[test]
fn coalesced_commits_refresh_and_checkpoint_without_another_frontend_call() {
    let fixture = Fixture::new();
    let mut database = Database::open_with_config(
        &fixture.0,
        DatabaseConfig {
            automatic_checkpoint_max_age: Duration::from_millis(20),
            ..DatabaseConfig::default()
        },
    )
    .unwrap();
    let suspension = database.runtime.suspend_automatic_checkpoint().unwrap();
    let control = database.runtime.checkpoint_control_for_test();
    for id in 0..32 {
        append(&mut database, id);
        let state = control.lock().unwrap();
        let latest = state.latest.as_ref().unwrap();
        assert_eq!(latest.store.node_count_for_label(None), 1);
        assert!(state.debt.unwrap().commit_epoch >= latest.store.commit_epoch());
    }
    drop(suspension);
    // Status-only observation cannot refresh a source or adopt on behalf of
    // the owner. All acknowledged rows must become a complete checkpoint.
    wait(&control, |state| {
        state.phase == Phase::Idle
            && state.report.completed_checkpoints > 0
            && state
                .debt
                .is_some_and(|debt| debt.commit_epoch == debt.checkpoint_commit_epoch)
    });
    verify(&mut database, 32);
    drop(database);
    verify(&mut Database::open(&fixture.0).unwrap(), 32);
}

#[test]
fn a_shared_read_guard_allows_refresh_of_a_suffix_after_prefix_sealing() {
    let fixture = Fixture::new();
    let mut database = Database::open_with_config(
        &fixture.0,
        DatabaseConfig {
            automatic_checkpoint_max_age: Duration::from_millis(20),
            ..DatabaseConfig::default()
        },
    )
    .unwrap();
    let suspension = database.runtime.suspend_automatic_checkpoint().unwrap();
    append(&mut database, 0);
    let control = database.runtime.checkpoint_control_for_test();
    let (sealed, observation) = mpsc::channel();
    let (resume, continuation) = mpsc::channel();
    control.lock().unwrap().prefix_seal_probe = Some(Arc::new(PrefixSealProbe {
        sealed,
        resume: Mutex::new(continuation),
    }));
    drop(suspension);
    observation.recv_timeout(Duration::from_secs(15)).unwrap();
    for id in 1..33 {
        append(&mut database, id);
    }
    {
        let state = control.lock().unwrap();
        assert!(
            state.latest.as_ref().unwrap().store.commit_epoch() < state.debt.unwrap().commit_epoch
        );
    }
    let read = database.runtime.get().unwrap();
    control.lock().unwrap().prefix_seal_probe = None;
    resume.send(()).unwrap();
    // The retained shared guard deliberately prevents handoff adoption,
    // while permitting immutable capture and full-prefix publication.
    wait(&control, |state| state.phase == Phase::Handoff);
    {
        let state = control.lock().unwrap();
        let selected = state.selected.as_ref().unwrap();
        assert_eq!(selected.expected, state.last_identity.unwrap());
        assert_eq!(selected.store.node_count_for_label(None), 33);
    }
    drop(read);
    wait(&control, |state| {
        state.phase == Phase::Idle && !state.report.waiting_for_handoff
    });
    verify(&mut database, 33);
    drop(database);
    verify(&mut Database::open(&fixture.0).unwrap(), 33);
}

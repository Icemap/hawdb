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

use hawdb::{
    Database, DurabilityPolicy, PreparedReadLimits, ProjectionGenerationBatchLimits,
    ProjectionGenerationBegin, ProjectionGenerationDigestBuilder, ProjectionGenerationError,
    ProjectionGenerationIdentity, ProjectionGenerationMember, ProjectionGenerationStore,
    ProjectionPublicationError, SealedProjectionGeneration, Value,
};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

struct TestRoot(PathBuf);
impl TestRoot {
    fn new() -> Self {
        static NEXT_ROOT: AtomicUsize = AtomicUsize::new(0);
        Self(std::env::temp_dir().join(format!(
            "hawdb_mvcc_publication_{}_{}_{}",
                std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )))
    }
}
impl Drop for TestRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn limits() -> PreparedReadLimits {
    PreparedReadLimits {
        dependency_entries: NonZeroUsize::new(32).unwrap(),
        dependency_bytes: NonZeroUsize::new(4096).unwrap(),
        statement_rows: NonZeroUsize::new(32).unwrap(),
        statement_payload_bytes: NonZeroUsize::new(4096).unwrap(),
    }
}

fn fixture(policy: DurabilityPolicy) -> (TestRoot, Database) {
    let root = TestRoot::new();
    let mut db = Database::open_with_durability(&root.0, policy)
        .unwrap_or_else(|error| panic!("opening fixture {}: {error}", root.0.display()));
    db.query("CREATE (:Memory {id: 'a', value: 1})").unwrap();
    db.query("CREATE (:Other {id: 'b', value: 1})").unwrap();
    (root, db)
}

fn read(db: &Database) -> hawdb::PreparedReadDependencies {
    let mut read = db.begin_prepared_read(limits()).unwrap();
    let parameters = BTreeMap::from([("id".into(), Value::String("a".into()))]);
    let output = read
        .query_with_params(
            "MATCH (n:Memory) WHERE n.id = $id RETURN n.value AS value",
            &parameters,
        )
        .unwrap();
    assert_eq!(output.output.rows.len(), 1);
    let dependencies = read.finish().unwrap();
    assert!(!dependencies.requires_unchanged_database_epoch());
    dependencies
}

fn seal(
    store: &ProjectionGenerationStore,
    generation: &str,
    epoch: u64,
    expected: Option<&str>,
) -> SealedProjectionGeneration {
    let mut writer = store
        .begin_candidate(
            ProjectionGenerationBegin {
                identity: ProjectionGenerationIdentity {
                    projection: "test_mvcc".into(),
                    owner_key: b"workspace".to_vec(),
                    generation: generation.into(),
                },
                source_watermark: epoch,
                projection_version: 1,
                expected_head: expected.map(str::to_owned),
            },
            ProjectionGenerationBatchLimits::default(),
        )
        .unwrap();
    let member = ProjectionGenerationMember {
        collection: "values".into(),
        key: b"a".to_vec(),
        payload: b"one".to_vec(),
    };
    writer.append_batch(std::slice::from_ref(&member)).unwrap();
    let mut digest = ProjectionGenerationDigestBuilder::default();
    digest.update(&member).unwrap();
    writer.seal(digest.finish()).unwrap()
}

#[test]
fn disjoint_writer_commits_while_query_preparation_is_paused() {
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
    let mut preparation = db.begin_prepared_read(limits()).unwrap();
    let store = db.projection_generation_store().unwrap();
    let dependencies = std::thread::scope(|scope| {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker = scope.spawn(move || {
            let output = preparation
                .query_with_params("MATCH (n:Memory) RETURN n.id AS id", &BTreeMap::new())
                .unwrap();
            assert_eq!(output.output.rows.len(), 1);
            ready_tx.send(()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            preparation.finish().unwrap()
        });
        ready_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let epoch = db.commit_epoch().unwrap();
        db.query("MATCH (n:Other) SET n.value = 2").unwrap();
        assert!(db.commit_epoch().unwrap() > epoch);
        resume_tx.send(()).unwrap();
        worker.join().unwrap()
    });
    let staged = seal(&store, "g1", dependencies.source_epoch(), None);
    assert!(matches!(
        store.open_active("test_mvcc", b"workspace"),
        Err(ProjectionGenerationError::NotFound(_))
    ));
    let report = db
        .publish_prepared_projection(&dependencies, &staged)
        .unwrap();
    assert!(!report.idempotent);
    assert_eq!(
        store
            .open_active("test_mvcc", b"workspace")
            .unwrap()
            .manifest(),
        staged.manifest()
    );
}

#[test]
fn relevant_write_and_schema_change_reject_without_publishing() {
    for schema in [false, true] {
        let (_root, mut db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
        let dependencies = read(&db);
        let store = db.projection_generation_store().unwrap();
        let staged = seal(&store, "g1", dependencies.source_epoch(), None);
        db.query(if schema {
            "CREATE INDEX ON :Memory(id)"
        } else {
            "MATCH (n:Memory) SET n.value = 2"
        })
        .unwrap();
        assert!(matches!(
            db.publish_prepared_projection(&dependencies, &staged),
            Err(ProjectionPublicationError::Dependency(_))
        ));
        assert!(matches!(
            store.open_active("test_mvcc", b"workspace"),
            Err(ProjectionGenerationError::NotFound(_))
        ));
    }
}

#[test]
fn empty_predicate_result_still_detects_concurrent_insertion() {
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
    let mut preparation = db.begin_prepared_read(limits()).unwrap();
    let parameters = BTreeMap::from([("id".into(), Value::String("absent".into()))]);
    let output = preparation
        .query_with_params(
            "MATCH (n:Memory) WHERE n.id = $id RETURN n.id AS id",
            &parameters,
        )
        .unwrap();
    assert!(output.output.rows.is_empty());
    let dependencies = preparation.finish().unwrap();
    let store = db.projection_generation_store().unwrap();
    let staged = seal(&store, "g1", dependencies.source_epoch(), None);
    db.query("CREATE (:Memory {id: 'absent'})").unwrap();
    assert!(matches!(
        db.publish_prepared_projection(&dependencies, &staged),
        Err(ProjectionPublicationError::Dependency(_))
    ));
}

#[test]
fn target_head_cas_rejects_another_prepared_candidate() {
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
    let dependencies = read(&db);
    let store = db.projection_generation_store().unwrap();
    let first = seal(&store, "g1", dependencies.source_epoch(), None);
    let second = seal(&store, "g2", dependencies.source_epoch(), None);
    db.publish_prepared_projection(&dependencies, &first)
        .unwrap();
    assert!(matches!(
        db.publish_prepared_projection(&dependencies, &second),
        Err(ProjectionPublicationError::Generation(
            ProjectionGenerationError::Conflict(_)
        ))
    ));
    assert_eq!(
        store
            .open_active("test_mvcc", b"workspace")
            .unwrap()
            .manifest(),
        first.manifest()
    );
}

#[test]
fn reopen_invalidates_uncommitted_source_but_resolves_committed_lost_response() {
    for committed in [false, true] {
        let (root, mut db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
        let dependencies = read(&db);
        let store = db.projection_generation_store().unwrap();
        let staged = seal(&store, "g1", dependencies.source_epoch(), None);
        if committed {
            db.publish_prepared_projection(&dependencies, &staged)
                .unwrap();
            db.query("MATCH (n:Memory) SET n.value = 2").unwrap();
        }
        drop(db);
        let mut reopened = Database::open(&root.0).unwrap();
        let result = reopened.publish_prepared_projection(&dependencies, &staged);
        if committed {
            assert!(result.unwrap().idempotent);
        } else {
            assert!(matches!(
                result,
                Err(ProjectionPublicationError::SourceChanged)
            ));
        }
    }
}

#[test]
fn watermark_and_source_durability_refusals_leave_head_absent() {
    for relaxed in [false, true] {
        let (_root, mut db) = fixture(if relaxed {
            DurabilityPolicy::SyncOnCheckpoint
        } else {
            DurabilityPolicy::SyncOnEveryWrite
        });
        let dependencies = read(&db);
        let store = db.projection_generation_store().unwrap();
        let staged = seal(
            &store,
            "g1",
            dependencies.source_epoch() + u64::from(!relaxed),
            None,
        );
        let result = db.publish_prepared_projection(&dependencies, &staged);
        if relaxed {
            assert!(matches!(
                result,
                Err(ProjectionPublicationError::SourceNotDurable)
            ));
        } else {
            assert!(matches!(
                result,
                Err(ProjectionPublicationError::SourceEpochMismatch { .. })
            ));
        }
        assert!(matches!(
            store.open_active("test_mvcc", b"workspace"),
            Err(ProjectionGenerationError::NotFound(_))
        ));
    }
}

#[test]
fn ignored_budget_or_unsupported_statement_error_cannot_produce_source_proof() {
    let (_root, db) = fixture(DurabilityPolicy::SyncOnEveryWrite);
    let mut tiny = limits();
    tiny.statement_payload_bytes = NonZeroUsize::new(1).unwrap();
    let mut preparation = db.begin_prepared_read(tiny).unwrap();
    assert!(preparation
        .query_with_params("MATCH (n:Memory) RETURN n.id AS id", &BTreeMap::new())
        .is_err());
    assert!(preparation.finish().is_err());
    let mut unsupported = db.begin_prepared_read(limits()).unwrap();
    assert!(unsupported
        .query_with_params(
            "MATCH (n:Memory)-[:LINK]->(m:Memory) RETURN m.id AS id",
            &BTreeMap::new()
        )
        .is_err());
    assert!(unsupported.finish().is_err());
    // An adjacency predicate may hide beneath a node scan/filter instead of
    // appearing as a separate traversal operator. It needs its own collector.
    let mut predicate = db.begin_prepared_read(limits()).unwrap();
    assert!(predicate
        .query_with_params(
            "MATCH (n:Memory) WHERE NOT (n)-[:LINK]->(:Other) RETURN n.id AS id",
            &BTreeMap::new(),
        )
        .is_err());
    assert!(predicate.finish().is_err());
}

#[test]
fn explicit_source_barrier_allows_relaxed_publication_without_changing_future_commits() {
    use hawdb::ProjectionSourceDurability;
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnCheckpoint);
    let dependencies = read(&db);
    let store = db.projection_generation_store().unwrap();
    let first = seal(&store, "g1", dependencies.source_epoch(), None);
    assert!(matches!(
        db.publish_prepared_projection(&dependencies, &first),
        Err(ProjectionPublicationError::SourceNotDurable)
    ));
    db.publish_prepared_projection_with_source_durability(
        &dependencies,
        &first,
        ProjectionSourceDurability::Synchronize,
    )
    .unwrap();
    db.query("MATCH (n:Other) SET n.value = 2").unwrap();
    let later = read(&db);
    assert!(later.source_epoch() > dependencies.source_epoch());
    let second = seal(&store, "g2", later.source_epoch(), Some("g1"));
    assert!(matches!(
        db.publish_prepared_projection(&later, &second),
        Err(ProjectionPublicationError::SourceNotDurable)
    ));
    db.publish_prepared_projection_with_source_durability(
        &later,
        &second,
        ProjectionSourceDurability::Synchronize,
    )
    .unwrap();
    assert_eq!(
        store
            .open_active("test_mvcc", b"workspace")
            .unwrap()
            .manifest(),
        second.manifest()
    );
}

#[test]
fn reopening_with_sync_policy_does_not_certify_a_relaxed_wal_prefix() {
    use hawdb::ProjectionSourceDurability;
    let (root, db) = fixture(DurabilityPolicy::SyncOnCheckpoint);
    drop(db);
    let mut reopened = Database::open(&root.0).unwrap();
    let dependencies = read(&reopened);
    let store = reopened.projection_generation_store().unwrap();
    let staged = seal(&store, "g1", dependencies.source_epoch(), None);
    assert!(matches!(
        reopened.publish_prepared_projection(&dependencies, &staged),
        Err(ProjectionPublicationError::SourceNotDurable)
    ));
    reopened
        .publish_prepared_projection_with_source_durability(
            &dependencies,
            &staged,
            ProjectionSourceDurability::Synchronize,
        )
        .unwrap();
}

#[test]
fn completed_checkpoint_certifies_a_relaxed_source_prefix() {
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnCheckpoint);
    db.query("CHECKPOINT").unwrap();
    let dependencies = read(&db);
    let store = db.projection_generation_store().unwrap();
    let staged = seal(&store, "g1", dependencies.source_epoch(), None);
    db.publish_prepared_projection(&dependencies, &staged)
        .unwrap();
}

#[test]
fn dependency_conflict_is_rejected_before_synchronizing_source() {
    use hawdb::ProjectionSourceDurability;
    let (_root, mut db) = fixture(DurabilityPolicy::SyncOnCheckpoint);
    let dependencies = read(&db);
    let store = db.projection_generation_store().unwrap();
    let staged = seal(&store, "stale", dependencies.source_epoch(), None);
    db.query("MATCH (n:Memory) SET n.value = 2").unwrap();
    assert!(matches!(
        db.publish_prepared_projection_with_source_durability(
            &dependencies,
            &staged,
            ProjectionSourceDurability::Synchronize
        ),
        Err(ProjectionPublicationError::Dependency(_))
    ));
    let fresh = read(&db);
    let retry = seal(&store, "fresh", fresh.source_epoch(), None);
    assert!(matches!(
        db.publish_prepared_projection(&fresh, &retry),
        Err(ProjectionPublicationError::SourceNotDurable)
    ));
    assert!(matches!(
        store.open_active("test_mvcc", b"workspace"),
        Err(ProjectionGenerationError::NotFound(_))
    ));
}

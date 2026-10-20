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
use crate::SearchRebuildOptions;
use std::any::TypeId;
use std::num::NonZeroU64;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("hawdb-consumer-{}", generate_uuidv7().unwrap()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn database(&self) -> Database {
        Database::open(self.0.join("database")).unwrap()
    }

    fn registry_path(&self, project: &str) -> PathBuf {
        let root = self.0.join(project);
        let runtime_root =
            match hawdb_storage::branch_project::inspect_project_manifest(&root).unwrap() {
                hawdb_storage::branch_project::ProjectManifest::Branch(selector) => root
                    .join("branches")
                    .join(selector.main_branch_id().as_uuid().to_string())
                    .join("data"),
                _ => root,
            };
        runtime_root.join("projection_consumers.meta")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn id(value: &str) -> SearchProjectionConsumerId {
    SearchProjectionConsumerId::new(value).unwrap()
}
fn options(commits: u64) -> SearchProjectionConsumerOptions {
    SearchProjectionConsumerOptions::new(NonZeroU64::new(commits).unwrap())
}
fn initialize(snapshot: &mut DatabaseReadTransaction, index: &mut SearchIndex) -> Result<()> {
    snapshot
        .rebuild_search_projection(index, SearchRebuildOptions::default())
        .map(|_| ())
}
fn append(db: &mut Database, value: u64) {
    db.query(&format!(
        "CREATE (:Memory {{id: 'm{value}', content: 'document {value}'}})"
    ))
    .unwrap();
}
fn catch_up(
    db: &mut Database,
    consumer: &mut SearchProjectionConsumer,
) -> SearchProjectionConsumerResult<SearchProjectionConsumerCatchUpReport> {
    db.catch_up_search_projection_consumer(consumer, 32, 32, 8, |_, _| {
        Ok(SearchProjectionRelationalDelta::default())
    })
}

#[test]
fn search_projection_consumer_facade_preserves_owner_type_identity() {
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerId>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerId>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerOptions>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerOptions>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumer>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumer>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerState>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerState>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerRebuildReason>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerRebuildReason>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerStatus>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerStatus>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerReadiness>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerReadiness>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerCatchUpReport>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerCatchUpReport>()
    );
    assert_eq!(
        TypeId::of::<crate::SearchProjectionConsumerError>(),
        TypeId::of::<hawdb_search::projection_consumer::SearchProjectionConsumerError>()
    );
}

#[test]
fn creation_checkpoint_catch_up_and_reopen_use_real_receipts() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let epoch = db.runtime.get().unwrap().store.commit_epoch();
    let mut consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    assert_eq!(db.runtime.get().unwrap().store.commit_epoch(), epoch);
    assert_eq!(consumer.search_index().document_count(), 1);
    assert!(db
        .search_projection_consumer_readiness(&consumer, None)
        .unwrap()
        .is_ready());
    append(&mut db, 2);
    let report = catch_up(&mut db, &mut consumer).unwrap();
    assert_eq!(report.catch_up.applied_batch_count, 1);
    assert!(report.catch_up.complete);
    assert_eq!(consumer.search_index().document_count(), 2);
    assert_eq!(
        report.consumer.minimum_valid_consumer_commit_epoch,
        Some(db.runtime.get().unwrap().store.commit_epoch())
    );
    drop(consumer);
    db.checkpoint().unwrap();
    let identity = db
        .runtime
        .get()
        .unwrap()
        .store
        .search_projection_database_identity();
    drop(db);
    let mut db = fixture.database();
    assert_eq!(
        db.runtime
            .get()
            .unwrap()
            .store
            .search_projection_database_identity(),
        identity
    );
    let status = db.search_projection_consumer_status(&id("main")).unwrap();
    assert_eq!(status.state, State::Unverified);
    assert_eq!(status.minimum_valid_consumer_commit_epoch, None);
    let consumer = db
        .open_search_projection_consumer(&id("main"), fixture.0.join("projection"))
        .unwrap();
    assert_eq!(consumer.search_index().document_count(), 2);
    assert_eq!(
        db.search_projection_consumer_status(&id("main"))
            .unwrap()
            .state,
        State::Active
    );
}

#[test]
fn empty_source_epoch_zero_and_repeated_catch_up_are_valid() {
    let fixture = Fixture::new();
    // Build a genuinely empty epoch-zero kernel project, without the facade's
    // implicit system-schema transaction. Admission still uses real custody.
    let root = fixture.0.join("database");
    let mut db = Database {
        project_root_path: Some(root.clone()),
        ..Database::default()
    };
    {
        let mut runtime = db.runtime.get_mut().unwrap();
        runtime.store = crate::store::GraphStore::open(&root, &mut runtime.catalog).unwrap();
    }
    let selector = hawdb_storage::branch_project::ProjectSelector::new(
        hawdb_storage::branch_catalog::BranchId::new(generate_uuidv7().unwrap()).unwrap(),
        hawdb_storage::branch_catalog::BranchId::new(generate_uuidv7().unwrap()).unwrap(),
    )
    .unwrap();
    db.runtime
        .get()
        .unwrap()
        .store
        .reserve_branch_project_identity(selector)
        .unwrap();
    db.checkpoint().unwrap();
    db.initialize_main_branch(
        selector.project_id().as_uuid(),
        selector.main_branch_id().as_uuid(),
    )
    .unwrap();
    db.runtime
        .get()
        .unwrap()
        .store
        .publish_branch_project_selector(selector)
        .unwrap();
    db.runtime.get_mut().unwrap().store = crate::store::GraphStore::default();
    let project =
        hawdb_storage::branch_project::ProjectMetadata::open(&root, db.config.max_open_files)
            .unwrap()
            .into_capture();
    let revision = project.metadata().unwrap().main().metadata_revision;
    let admitted = crate::store::GraphStore::admit_captured_branch(
        hawdb_storage::store::CapturedBranchAdmissionRequest {
            project: &project,
            branch_id: selector.main_branch_id(),
            expected_metadata_revision: revision,
            durability: db.durability,
            replay_config: db.config.wal_replay_config(),
            read_only: false,
        },
    )
    .unwrap();
    let (store, catalog) = admitted.into_parts();
    {
        let mut runtime = db.runtime.get_mut().unwrap();
        runtime.store = store;
        runtime.catalog = catalog;
    }
    assert_eq!(db.runtime.get().unwrap().store.commit_epoch(), 0);
    let mut consumer = db
        .create_search_projection_consumer(
            id("empty"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    assert_eq!(consumer.projection().receipt().source_epoch, 0);
    let before = std::fs::read(fixture.registry_path("database")).unwrap();
    let report = catch_up(&mut db, &mut consumer).unwrap();
    assert!(report.catch_up.complete);
    assert_eq!(report.catch_up.applied_batch_count, 0);
    assert_eq!(
        before,
        std::fs::read(fixture.registry_path("database")).unwrap()
    );
}

#[test]
fn initialization_failure_cleans_stage_and_preserves_existing_destination() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let result = db.create_search_projection_consumer(
        id("failed"),
        fixture.0.join("projection"),
        options(100),
        |_, index| {
            assert!(index.is_persistent());
            Err(HawDBError::Execution("incomplete initialization".into()))
        },
    );
    assert!(result.is_err());
    assert!(!fixture.0.join("projection").exists());
    assert!(!std::fs::read_dir(&fixture.0).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".stage")));
    std::fs::create_dir(fixture.0.join("projection")).unwrap();
    std::fs::write(fixture.0.join("projection/keep"), "untouched").unwrap();
    let result = db.create_search_projection_consumer(
        id("failed"),
        fixture.0.join("projection"),
        options(100),
        |_, _| panic!("must not initialize existing destination"),
    );
    assert!(result.is_err());
    assert_eq!(
        std::fs::read(fixture.0.join("projection/keep")).unwrap(),
        b"untouched"
    );
}

#[test]
fn owned_projection_excludes_other_publishers_and_releases_lease_on_drop() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("owned"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    assert!(consumer.search_index().checkpoint().is_err());
    assert!(consumer
        .search_index()
        .mark_full_reindex_needed("external")
        .is_err());
    assert!(SearchIndex::open(fixture.0.join("projection")).is_err());
    assert!(db
        .open_search_projection_consumer(&id("owned"), fixture.0.join("projection"))
        .is_err());
    drop(consumer);
    assert!(SearchIndex::open(fixture.0.join("projection")).is_err());
    db.open_search_projection_consumer(&id("owned"), fixture.0.join("projection"))
        .unwrap();
}

#[test]
fn hard_retention_floor_invalidates_only_the_lagging_consumer() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.runtime
        .get_mut()
        .unwrap()
        .store
        .set_max_search_projection_change_log_entries(Some(1));
    let mut lagging = db
        .create_search_projection_consumer(
            id("lagging"),
            fixture.0.join("lagging"),
            options(100),
            initialize,
        )
        .unwrap();
    let mut current = db
        .create_search_projection_consumer(
            id("current"),
            fixture.0.join("current"),
            options(100),
            initialize,
        )
        .unwrap();
    append(&mut db, 1);
    let first_epoch = db.runtime.get().unwrap().store.commit_epoch();
    catch_up(&mut db, &mut current).unwrap();
    append(&mut db, 2);
    let status = db
        .search_projection_consumer_status(&id("lagging"))
        .unwrap();
    assert!(matches!(
        status.state,
        State::RebuildRequired(Reason::RetentionLimitExceeded { .. })
    ));
    assert_eq!(
        db.search_projection_consumer_status(&id("current"))
            .unwrap()
            .state,
        State::Active
    );
    assert_eq!(
        status.minimum_valid_consumer_commit_epoch,
        Some(first_epoch)
    );
    assert!(matches!(
        catch_up(&mut db, &mut lagging),
        Err(Error::RebuildRequired(
            Reason::RetentionLimitExceeded { .. }
        ))
    ));
    catch_up(&mut db, &mut current).unwrap();
    assert_eq!(
        db.runtime
            .get()
            .unwrap()
            .store
            .search_projection_changefeed_status()
            .retained_mutation_count,
        1
    );
}

#[test]
fn expiry_is_inclusive_renewal_is_explicit_and_stale_handles_stay_revoked() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut old = db
        .create_search_projection_consumer(
            id("reusable"),
            fixture.0.join("old"),
            options(2),
            initialize,
        )
        .unwrap();
    append(&mut db, 1);
    let deadline = db.runtime.get().unwrap().store.commit_epoch() + 2;
    assert_eq!(
        db.renew_search_projection_consumer(&old)
            .unwrap()
            .expires_at_commit_epoch,
        deadline
    );
    append(&mut db, 2);
    append(&mut db, 3);
    assert!(matches!(
        db.renew_search_projection_consumer(&old),
        Err(Error::RebuildRequired(Reason::Expired {
            expires_at_commit_epoch
        })) if expires_at_commit_epoch == deadline
    ));
    db.unregister_search_projection_consumer(&id("reusable"))
        .unwrap();
    db.unregister_search_projection_consumer(&id("reusable"))
        .unwrap();
    let _new = db
        .create_search_projection_consumer(
            id("reusable"),
            fixture.0.join("new"),
            options(100),
            initialize,
        )
        .unwrap();
    assert!(matches!(
        catch_up(&mut db, &mut old),
        Err(Error::InvalidHandle)
    ));
    assert!(fixture.0.join("old/search_projection.hawdb").exists());
}

#[test]
fn active_wal_sync_group_rejects_consumer_writes_before_callbacks() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut consumer = db
        .create_search_projection_consumer(
            id("owned"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    db.begin_wal_sync_group().unwrap();
    append(&mut db, 1);
    assert!(matches!(
        db.renew_search_projection_consumer(&consumer),
        Err(Error::SourceNotDurable)
    ));
    assert!(matches!(
        db.catch_up_search_projection_consumer(&mut consumer, 1, 1, 1, |_, _| panic!(
            "must reject before hydration"
        )),
        Err(Error::SourceNotDurable)
    ));
    assert!(matches!(
        db.create_search_projection_consumer(
            id("other"),
            fixture.0.join("other"),
            options(100),
            |_, _| panic!("must reject before initialization")
        ),
        Err(Error::SourceNotDurable)
    ));
    db.finish_wal_sync_group().unwrap();
    assert!(catch_up(&mut db, &mut consumer).unwrap().catch_up.complete);
}

#[test]
fn identity_survives_database_rename_and_rejects_copied_registry() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    drop(consumer);
    drop(db);
    std::fs::rename(fixture.0.join("database"), fixture.0.join("moved")).unwrap();
    let mut db = Database::open(fixture.0.join("moved")).unwrap();
    let consumer = db
        .open_search_projection_consumer(&id("main"), fixture.0.join("projection"))
        .unwrap();
    drop(consumer);
    drop(db);
    let replacement = fixture.database();
    drop(replacement);
    let registry = fixture.registry_path("database");
    std::fs::create_dir_all(registry.parent().unwrap()).unwrap();
    std::fs::copy(fixture.registry_path("moved"), registry).unwrap();
    let mut replacement = fixture.database();
    assert!(matches!(
        replacement.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
        Err(Error::RebuildRequired(Reason::DatabaseIdentityMismatch))
    ));
}

#[test]
fn registry_publication_error_never_adopts_the_unacknowledged_checkpoint() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    let old = std::fs::read(fixture.registry_path("database")).unwrap();
    append(&mut db, 1);
    PUBLICATION_FAILURE.set(Some((PublicationStage::BeforeRegistry, false)));
    assert!(catch_up(&mut db, &mut consumer).is_err());
    assert_eq!(
        std::fs::read(fixture.registry_path("database")).unwrap(),
        old
    );
    assert!(!fixture
        .registry_path("database")
        .with_file_name("projection_consumers.meta.tmp")
        .exists());
    assert_eq!(
        consumer
            .search_index()
            .projection_freshness()
            .durable_source_graph_commit_epoch,
        Some(db.runtime.get().unwrap().store.commit_epoch())
    );
    drop(consumer);
    drop(db);
    let mut db = fixture.database();
    assert!(matches!(
        db.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
        Err(Error::RebuildRequired(Reason::CheckpointMismatch))
    ));
}

#[test]
fn missing_and_corrupt_registry_leave_ordinary_database_writes_available() {
    for corrupt in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        let consumer = db
            .create_search_projection_consumer(
                id("main"),
                fixture.0.join("projection"),
                options(100),
                initialize,
            )
            .unwrap();
        drop(consumer);
        drop(db);
        let registry = fixture.registry_path("database");
        if corrupt {
            std::fs::write(&registry, b"corrupt").unwrap();
        } else {
            std::fs::remove_file(&registry).unwrap();
        }
        let mut db = fixture.database();
        assert!(matches!(
            db.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
            Err(Error::RebuildRequired(Reason::RegistryUnavailable))
        ));
        append(&mut db, 1);
        let replacement = db
            .create_search_projection_consumer(
                id("main"),
                fixture.0.join("rebuilt"),
                options(100),
                initialize,
            )
            .unwrap();
        assert_eq!(replacement.search_index().document_count(), 1);
    }
}

#[test]
fn overflow_and_hydration_errors_do_not_publish_progress() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    assert!(db
        .create_search_projection_consumer(
            id("overflow"),
            fixture.0.join("overflow"),
            options(u64::MAX),
            |_, _| panic!("must reject overflow before initialization")
        )
        .is_err());
    let mut consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    append(&mut db, 1);
    let receipt = consumer.projection().receipt();
    let before = std::fs::read(fixture.registry_path("database")).unwrap();
    let result = db.catch_up_search_projection_consumer(&mut consumer, 1, 1, 1, |_, _| {
        Err(HawDBError::Execution("incomplete hydration".into()))
    });
    assert!(result.is_err());
    assert_eq!(consumer.projection().receipt(), receipt);
    assert_eq!(
        std::fs::read(fixture.registry_path("database")).unwrap(),
        before
    );
    assert_eq!(
        db.search_projection_consumer_status(&id("main"))
            .unwrap()
            .state,
        State::Active
    );
}

#[test]
fn consumer_crash_child() {
    let Ok(root) = std::env::var("HAWDB_CONSUMER_CRASH_ROOT") else {
        return;
    };
    let root = Path::new(&root);
    let mut db = Database::open(root.join("database")).unwrap();
    let mut consumer = db
        .open_search_projection_consumer(&id("main"), root.join("projection"))
        .unwrap();
    let stage = match std::env::var("HAWDB_CONSUMER_CRASH_STAGE")
        .unwrap()
        .as_str()
    {
        "before_checkpoint" => PublicationStage::BeforeCheckpoint,
        "after_checkpoint" => PublicationStage::AfterCheckpoint,
        "after_registry" => PublicationStage::AfterRegistry,
        _ => panic!("unknown crash stage"),
    };
    PUBLICATION_FAILURE.set(Some((stage, true)));
    catch_up(&mut db, &mut consumer).unwrap();
    panic!("crash failpoint was not reached");
}

#[test]
fn process_crashes_observe_checkpoint_before_registry_ordering() {
    for stage in ["before_checkpoint", "after_checkpoint", "after_registry"] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        let consumer = db
            .create_search_projection_consumer(
                id("main"),
                fixture.0.join("projection"),
                options(100),
                initialize,
            )
            .unwrap();
        let old_epoch = consumer.projection().receipt().source_epoch;
        append(&mut db, 1);
        let source_epoch = db.runtime.get().unwrap().store.commit_epoch();
        drop(consumer);
        drop(db);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "api::search_projection_consumer::tests::consumer_crash_child",
                "--nocapture",
            ])
            .env("HAWDB_CONSUMER_CRASH_ROOT", &fixture.0)
            .env("HAWDB_CONSUMER_CRASH_STAGE", stage)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(86));
        let mut db = fixture.database();
        let opened = db.open_search_projection_consumer(&id("main"), fixture.0.join("projection"));
        if stage == "after_checkpoint" {
            assert!(matches!(
                opened,
                Err(Error::RebuildRequired(Reason::CheckpointMismatch))
            ));
        } else {
            let mut consumer = opened.unwrap();
            assert_eq!(
                consumer.projection().receipt().source_epoch,
                if stage == "before_checkpoint" {
                    old_epoch
                } else {
                    source_epoch
                }
            );
            assert!(catch_up(&mut db, &mut consumer).unwrap().catch_up.complete);
            assert_eq!(consumer.search_index().document_count(), 1);
        }
    }
}

#[test]
fn missing_mandatory_projection_artifact_requires_rebuild_without_repair() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    drop(consumer);
    drop(db);
    let descriptor = fixture
        .0
        .join("projection/search_projection_segments.hawdb");
    assert!(descriptor.exists());
    std::fs::remove_file(&descriptor).unwrap();
    let mut db = fixture.database();
    assert!(matches!(
        db.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
        Err(Error::RebuildRequired(Reason::CheckpointMismatch))
    ));
    assert!(!descriptor.exists());
}

#[test]
fn failed_registration_retry_preserves_other_durable_consumers() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let first = db
        .create_search_projection_consumer(
            id("first"),
            fixture.0.join("first"),
            options(100),
            initialize,
        )
        .unwrap();
    PUBLICATION_FAILURE.set(Some((PublicationStage::BeforeRegistry, false)));
    assert!(db
        .create_search_projection_consumer(
            id("failed"),
            fixture.0.join("failed"),
            options(100),
            initialize
        )
        .is_err());
    let second = db
        .create_search_projection_consumer(
            id("second"),
            fixture.0.join("second"),
            options(100),
            initialize,
        )
        .unwrap();
    assert_eq!(
        db.runtime.get().unwrap().projection_consumers.records.len(),
        2
    );
    assert!(db
        .runtime
        .get()
        .unwrap()
        .projection_consumers
        .records
        .contains_key("first"));
    db.renew_search_projection_consumer(&first).unwrap();
    assert_eq!(
        db.search_projection_consumer_status(&id("first"))
            .unwrap()
            .state,
        State::Active
    );
    drop(second);
}

#[test]
fn consumer_registry_capacity_is_reclaimed_only_by_unregister() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    for index in 0..MAX_CONSUMERS {
        let name = format!("consumer-{index:02}");
        db.create_search_projection_consumer(
            id(&name),
            fixture.0.join(&name),
            options(1),
            initialize,
        )
        .unwrap();
    }
    append(&mut db, 1);
    assert!(matches!(
        db.search_projection_consumer_status(&id("consumer-00"))
            .unwrap()
            .state,
        State::RebuildRequired(Reason::Expired { .. })
    ));
    let result = db.create_search_projection_consumer(
        id("overflow"),
        fixture.0.join("overflow"),
        options(1000),
        |_, _| panic!("full registry must reject before initialization"),
    );
    assert!(matches!(result, Err(Error::RegistryFull)));
    db.unregister_search_projection_consumer(&id("consumer-00"))
        .unwrap();
    db.create_search_projection_consumer(
        id("replacement"),
        fixture.0.join("replacement"),
        options(1000),
        initialize,
    )
    .unwrap();
    assert_eq!(
        db.runtime.get().unwrap().projection_consumers.records.len(),
        MAX_CONSUMERS
    );
    assert!(
        std::fs::metadata(fixture.registry_path("database"))
            .unwrap()
            .len()
            <= 64 * 1024
    );
}

#[test]
fn acknowledgement_is_idempotent_monotonic_and_bound_to_the_complete_receipt() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    let receipt = consumer.projection().receipt();
    db.acknowledge_consumer_checkpoint(consumer.id(), &receipt)
        .unwrap();
    for variation in 0..5 {
        let mut invalid = receipt.clone();
        match variation {
            0 => invalid.source_epoch = db.runtime.get().unwrap().store.commit_epoch() + 1,
            1 => invalid.source_epoch -= 1,
            2 => invalid.sha256 = "0".repeat(64),
            3 => invalid.binding.checkpoint_uuid = generate_uuidv7().unwrap(),
            _ => invalid.binding.registration_uuid = generate_uuidv7().unwrap(),
        }
        assert!(matches!(
            db.acknowledge_consumer_checkpoint(consumer.id(), &invalid),
            Err(Error::RebuildRequired(Reason::CheckpointMismatch))
        ));
    }
    assert_eq!(
        db.search_projection_consumer_status(consumer.id())
            .unwrap()
            .durable_complete_through_commit_epoch,
        Some(receipt.source_epoch)
    );
}

#[test]
fn byte_retention_limit_does_not_wait_for_registry_io() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    db.runtime
        .get_mut()
        .unwrap()
        .store
        .set_max_search_projection_change_log_entries(None);
    db.runtime
        .get_mut()
        .unwrap()
        .store
        .set_max_search_projection_change_log_bytes(Some(1));
    let registry = fixture.registry_path("database");
    let old = std::fs::read(&registry).unwrap();
    append(&mut db, 1);
    let status = db.search_projection_consumer_status(consumer.id()).unwrap();
    assert!(status.changefeed.retained_bytes <= 1);
    assert!(matches!(
        status.state,
        State::RebuildRequired(Reason::RetentionLimitExceeded { .. })
    ));
    assert_eq!(std::fs::read(&registry).unwrap(), old);
    let readiness = db
        .search_projection_consumer_readiness(&consumer, None)
        .unwrap();
    assert!(!readiness.is_ready());
    assert_eq!(readiness.consumer.state, status.state);
    // A complete fresh-C replacement can retain this valid target without
    // pretending the discarded delta interval is available again.
    drop(
        db.reserve_search_projection_consumer(&mut consumer)
            .unwrap(),
    );
    assert_eq!(std::fs::read(&registry).unwrap(), old);
    assert!(matches!(
        catch_up(&mut db, &mut consumer),
        Err(Error::RebuildRequired(
            Reason::RetentionLimitExceeded { .. }
        ))
    ));
}

#[test]
fn backup_restores_identity_and_rejects_a_cursor_ahead_of_the_restored_source() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    let identity = db
        .runtime
        .get()
        .unwrap()
        .store
        .search_projection_database_identity();
    db.backup_to(fixture.0.join("backup")).unwrap();
    append(&mut db, 1);
    catch_up(&mut db, &mut consumer).unwrap();
    drop(consumer);
    Database::restore_backup(fixture.0.join("backup"), fixture.0.join("restored")).unwrap();
    let mut restored = Database::open(fixture.0.join("restored")).unwrap();
    assert_eq!(
        restored
            .runtime
            .get()
            .unwrap()
            .store
            .search_projection_database_identity(),
        identity
    );
    assert!(matches!(
        restored.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
        Err(Error::RebuildRequired(Reason::RegistryUnavailable))
    ));
    drop(restored);
    std::fs::copy(
        fixture.registry_path("database"),
        fixture.registry_path("restored"),
    )
    .unwrap();
    let mut restored = Database::open(fixture.0.join("restored")).unwrap();
    assert!(matches!(
        restored.open_search_projection_consumer(&id("main"), fixture.0.join("projection")),
        Err(Error::RebuildRequired(Reason::SourceRewound))
    ));
}

#[test]
fn foreign_registry_reinitializes_only_after_the_new_projection_is_complete() {
    let fixture = Fixture::new();
    let mut source = fixture.database();
    append(&mut source, 1);
    let original = source
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("original"),
            options(100),
            initialize,
        )
        .unwrap();
    let source_identity = source
        .runtime
        .get()
        .unwrap()
        .store
        .search_projection_database_identity();
    let mut replacement = Database::open(fixture.0.join("replacement-database")).unwrap();
    append(&mut replacement, 2);
    drop(replacement);
    let foreign_bytes = std::fs::read(fixture.registry_path("database")).unwrap();
    let registry = fixture.registry_path("replacement-database");
    std::fs::write(&registry, &foreign_bytes).unwrap();
    let mut replacement = Database::open(fixture.0.join("replacement-database")).unwrap();
    let epoch = replacement.commit_epoch();
    assert!(matches!(
        replacement
            .search_projection_consumer_status(&id("main"))
            .unwrap()
            .state,
        State::RebuildRequired(Reason::DatabaseIdentityMismatch)
    ));
    assert!(replacement
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("failed"),
            options(100),
            |_, _| Err(HawDBError::Execution("incomplete mapping".into()))
        )
        .is_err());
    assert_eq!(std::fs::read(&registry).unwrap(), foreign_bytes);
    assert!(matches!(
        replacement
            .search_projection_consumer_status(&id("main"))
            .unwrap()
            .state,
        State::RebuildRequired(Reason::DatabaseIdentityMismatch)
    ));
    let rebuilt = replacement
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("rebuilt"),
            options(100),
            initialize,
        )
        .unwrap();
    assert_eq!(replacement.commit_epoch(), epoch);
    assert_ne!(
        replacement
            .runtime
            .get()
            .unwrap()
            .store
            .search_projection_database_identity(),
        source_identity
    );
    assert!(rebuilt.search_index().document("memory:m2").is_some());
    assert!(rebuilt.search_index().document("memory:m1").is_none());
    assert!(matches!(
        replacement.renew_search_projection_consumer(&original),
        Err(Error::RebuildRequired(Reason::DatabaseIdentityMismatch))
    ));
    drop(rebuilt);
    drop(replacement);
    let mut replacement = Database::open(fixture.0.join("replacement-database")).unwrap();
    replacement
        .open_search_projection_consumer(&id("main"), fixture.0.join("rebuilt"))
        .unwrap();
}

#[test]
fn stage_cleanup_preserves_unpublished_files_until_descriptor_capacity_returns() {
    use hawdb_storage::file_descriptors::ProjectFileDescriptors;
    use hawdb_storage::file_io as counted;
    let fixture = Fixture::new();
    let project = ProjectFileDescriptors::acquire(&fixture.0, 4).unwrap();
    for explicit in [true, false] {
        let stage = fixture.0.join(format!("stage-{explicit}"));
        counted::create_dir(&stage).unwrap();
        let evidence = stage.join("unpublished");
        counted::write(&evidence, b"retain unpublished bytes").unwrap();
        let mut held = (0..4)
            .map(|_| counted::File::open(&evidence).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(project.metrics().open, 4);
        let rejections = project.metrics().budget_rejections;
        let guard = StageDirectory(stage.clone());
        if explicit {
            assert!(matches!(
                guard.cleanup(),
                Err(HawDBError::FileDescriptors(_))
            ));
        } else {
            drop(guard);
        }
        assert_eq!(
            std::fs::read(&evidence).unwrap(),
            b"retain unpublished bytes"
        );
        assert!(project.metrics().budget_rejections > rejections);
        assert_eq!(project.metrics().open, 4);
        assert_eq!(project.metrics().reserved, 0);
        drop(held.pop());
        StageDirectory(stage.clone()).cleanup().unwrap();
        assert!(!stage.exists());
        assert_eq!(project.metrics().open, 3);
        drop(held);
    }
    assert_eq!(project.metrics().open, 0);
    assert!(project.metrics().high_water <= 4);
}

#[test]
fn initializer_descriptor_rejection_preserves_typed_cause_and_source() {
    use hawdb_storage::file_io as counted;
    let fixture = Fixture::new();
    let root = fixture.0.join("database");
    let mut db = Database::open_with_config(
        &root,
        crate::DatabaseConfig {
            max_open_files: 32,
            ..crate::DatabaseConfig::default()
        },
    )
    .unwrap();
    append(&mut db, 1);
    let epoch = db.commit_epoch().unwrap();
    let branch = db.current_branch().unwrap().unwrap().info.id;
    let destination = root.join("projection");
    let project =
        hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire_existing(&root, 32)
            .unwrap();
    let mut held = Vec::new();
    let mut evidence = None;
    let error = db
        .create_search_projection_consumer(id("fd-retry"), &destination, options(100), |_, _| {
            let stage = std::fs::read_dir(&root)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .find(|path| {
                    path.file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .ends_with(".stage")
                })
                .unwrap();
            let file = stage.join("unpublished");
            counted::write(&file, b"retain initializer evidence").unwrap();
            let error = loop {
                match counted::File::open(&file) {
                    Ok(file) => held.push(file),
                    Err(error) => break HawDBError::from(error),
                }
            };
            assert!(matches!(error, HawDBError::FileDescriptors(_)));
            assert_eq!(project.metrics().open, 32);
            evidence = Some(file);
            Err(error)
        })
        .unwrap_err();
    assert!(
        matches!(error, Error::Database(HawDBError::FileDescriptors(_))),
        "expected a typed FD failure, got {error:?}"
    );
    let evidence = evidence.unwrap();
    // Failed initialization releases its publication lock before cleanup;
    // that one returned descriptor lets the bounded walker remove the stage.
    assert!(!evidence.exists());
    assert!(!destination.exists());
    assert!(db.file_descriptor_metrics().unwrap().open < 32);
    assert_eq!(db.file_descriptor_metrics().unwrap().reserved, 0);
    drop(held);
    assert_eq!(db.commit_epoch().unwrap(), epoch);
    assert_eq!(db.current_branch().unwrap().unwrap().info.id, branch);
    assert!(db
        .query("MATCH (m:Memory) RETURN m.id AS id")
        .unwrap()
        .rows
        .iter()
        .any(|row| row["id"] == crate::Value::String("m1".into())));
    let consumer = db
        .create_search_projection_consumer(id("fd-retry"), &destination, options(100), initialize)
        .unwrap();
    assert!(consumer.search_index().document("memory:m1").is_some());
    assert!(db
        .search_projection_consumer_readiness(&consumer, None)
        .unwrap()
        .is_ready());
    assert!(db.file_descriptor_metrics().unwrap().high_water <= 32);
    drop(consumer);
    drop(db);
    let mut reopened = Database::open_with_config(
        &root,
        crate::DatabaseConfig {
            max_open_files: 32,
            ..crate::DatabaseConfig::default()
        },
    )
    .unwrap();
    let consumer = reopened
        .open_search_projection_consumer(&id("fd-retry"), &destination)
        .unwrap();
    assert!(consumer.search_index().document("memory:m1").is_some());
}

#[test]
fn reserved_target_defers_catch_up_before_hydration_and_allows_source_writes() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut consumer = db
        .create_search_projection_consumer(
            id("reserved"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    let preparation = db
        .reserve_search_projection_consumer(&mut consumer)
        .unwrap();
    append(&mut db, 2);
    let hydrated = std::cell::Cell::new(false);
    let result = db.catch_up_search_projection_consumer(&mut consumer, 32, 32, 8, |_, _| {
        hydrated.set(true);
        Ok(SearchProjectionRelationalDelta::default())
    });
    assert!(matches!(
        result,
        Err(SearchProjectionConsumerError::Database(
            HawDBError::ProjectionBusy
        ))
    ));
    assert!(!hydrated.get());
    assert_eq!(consumer.search_index().document_count(), 1);
    drop(preparation);
    assert!(catch_up(&mut db, &mut consumer).unwrap().catch_up.complete);
    assert_eq!(consumer.search_index().document_count(), 2);
}

#[test]
fn expired_feed_allows_fresh_preparation_without_restoring_delta_readiness() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut consumer = db
        .create_search_projection_consumer(
            id("expired-preparation"),
            fixture.0.join("projection"),
            options(1),
            initialize,
        )
        .unwrap();
    append(&mut db, 2);
    append(&mut db, 3);
    assert!(matches!(
        db.search_projection_consumer_status(consumer.id())
            .unwrap()
            .state,
        State::RebuildRequired(Reason::Expired { .. })
    ));
    let operation = db
        .reserve_search_projection_consumer(&mut consumer)
        .unwrap();
    append(&mut db, 4);
    drop(operation);
    assert!(matches!(
        catch_up(&mut db, &mut consumer),
        Err(Error::RebuildRequired(Reason::Expired { .. }))
    ));
    assert_eq!(consumer.search_index().document_count(), 1);
    assert!(matches!(
        db.search_projection_consumer_status(consumer.id())
            .unwrap()
            .state,
        State::RebuildRequired(Reason::Expired { .. })
    ));
}

fn allocation_evidence(fixture: &Fixture) -> (Vec<u8>, serde_json::Value) {
    let bytes = std::fs::read(fixture.0.join("database/consumer-allocations.hawdb")).unwrap();
    let start = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
    let value = serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap();
    (bytes, value)
}

fn reserve_native_generation(
    db: &mut Database,
    fixture: &Fixture,
) -> hawdb_storage::consumer_allocation::ReservedConsumerAllocation {
    if db
        .runtime
        .get()
        .unwrap()
        .store
        .search_projection_database_identity()
        .is_none()
    {
        db.runtime
            .get_mut()
            .unwrap()
            .store
            .set_search_projection_database_identity(generate_uuidv7().unwrap());
        db.checkpoint().unwrap();
    }
    db.runtime
        .get()
        .unwrap()
        .store
        .reserve_search_consumer_allocation("stationary", &fixture.0.join("target"))
        .unwrap()
}

#[test]
fn consumer_native_birth_requires_original_creation_and_retains_receipt_across_close() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let name = pending.generation_name().to_owned();
    assert!(!fixture.0.join(&name).exists());
    assert!(pending.born_directory().is_err());
    let evidence = allocation_evidence(&fixture).1;
    assert_eq!(evidence["pending"][0]["native_birth"]["state"], "Pending");
    assert_eq!(
        evidence["pending"][0]["native_birth"]["generation_name"],
        name
    );
    pending.create().unwrap();
    let directory = pending.born_directory().unwrap();
    let context = directory.operation_context();
    let marker = directory
        .open_child(&context, "consumer-birth.hawdb")
        .unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut &marker, &mut bytes).unwrap();
    assert!(bytes.starts_with(b"HAWDB_CONSUMER_BIRTH_V1\n"));
    let checksum_at = bytes.len() - 4;
    assert_eq!(
        hawdb_integrity::crc32c(&bytes[..checksum_at]).get(),
        u32::from_le_bytes(bytes[checksum_at..].try_into().unwrap())
    );
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
    // A duplicate call must not re-run mkdir or overwrite the marker.
    assert!(pending.create().is_err());
    drop(marker);
    drop(pending);
    drop(db);
    let mut db = fixture.database();
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 3);
    assert!(fixture.0.join(&name).join("consumer-birth.hawdb").exists());
    let next = reserve_native_generation(&mut db, &fixture);
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 6);
    let next = next.prepare_stationary_generation(&fixture.0).unwrap();
    assert_ne!(next.generation_name(), name);
    assert!(next.born_directory().is_err());
    // Registry/readiness has not been granted by recording a private birth.
    assert!(db
        .runtime
        .get()
        .unwrap()
        .projection_consumers
        .records
        .is_empty());
}

#[test]
fn consumer_native_birth_target_lease_excludes_a_second_publication_owner() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let original = pending.born_directory().unwrap().identity();
    let task = hawdb_core::RuntimeTaskContext::default();
    let lease = hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
        .unwrap();
    assert_eq!(lease.source_identity(), original);
    let error = ConsumerProjection::open(&generation).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("another search projection publication is active"),
        "{error}"
    );
    assert!(generation.join("consumer-birth.hawdb").exists());
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
    drop(lease);
    assert_eq!(pending.born_directory().unwrap().identity(), original);
}

fn candidate_reference(bytes: &[u8]) -> hawdb_storage::immutable_object::ObjectReference {
    hawdb_storage::immutable_object::ObjectReference::for_bytes(
        hawdb_storage::immutable_object::ObjectKind::CheckpointArtifact,
        1,
        bytes,
    )
}

#[test]
fn consumer_native_selector_retains_previous_root_and_historical_receipt() {
    use hawdb_storage::consumer_allocation::{
        ConsumerSelectionResolution, PendingConsumerGeneration,
    };

    fn sealed_birth(
        db: &mut Database,
        fixture: &Fixture,
        bytes: &[u8],
    ) -> PendingConsumerGeneration {
        let epoch = db.begin_read_transaction().unwrap().commit_epoch();
        let mut birth = reserve_native_generation(db, fixture)
            .prepare_stationary_generation(&fixture.0)
            .unwrap();
        birth.create().unwrap();
        let directory = birth.born_directory().unwrap();
        let context = directory.operation_context();
        let mut file = directory
            .create_child_new(&context, "consumer-generation.meta.hawdb")
            .unwrap();
        std::io::Write::write_all(&mut file, bytes).unwrap();
        file.sync_all().unwrap();
        directory.sync(&context).unwrap();
        drop(file);
        birth
            .record_candidate_seal(epoch, candidate_reference(bytes))
            .unwrap();
        birth.record_publication_intent().unwrap();
        birth
    }

    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut first = sealed_birth(&mut db, &fixture, b"first metadata");
    let first_path = fixture.0.join(first.generation_name());
    let mut first_selector = first.prepare_selection().unwrap();
    let admission = first_selector.reserve_attempt_descriptors().unwrap();
    let first_receipt = first_selector.select().unwrap().finish().unwrap();
    drop(admission);
    append(&mut db, 2);
    let mut second = sealed_birth(&mut db, &fixture, b"second metadata");
    let second_path = fixture.0.join(second.generation_name());
    let evidence = allocation_evidence(&fixture).1;
    assert_eq!(
        evidence["pending"][1]["publication_intent"]["previous"]["operation"],
        first_receipt.binding().operation.get()
    );
    let mut second_selector = second.prepare_selection().unwrap();
    let admission = second_selector.reserve_attempt_descriptors().unwrap();
    drop(second_selector.select().unwrap());
    drop(admission);
    append(&mut db, 3);
    let admission = second_selector.reserve_attempt_descriptors().unwrap();
    let ConsumerSelectionResolution::Published(second_receipt) = second_selector.resolve().unwrap()
    else {
        panic!("second native selection was lost");
    };
    assert_eq!(
        second_receipt.metadata_reference(),
        candidate_reference(b"second metadata")
    );
    assert_eq!(
        first_selector.resolve().unwrap(),
        ConsumerSelectionResolution::Published(Box::new(first_receipt.clone()))
    );
    drop(admission);
    drop(first_selector);
    drop(second_selector);
    drop(first);
    drop(second);
    drop(db);
    let reopened = fixture.database();
    let evidence = allocation_evidence(&fixture).1;
    assert_eq!(
        evidence["selected"][0]["operation"],
        second_receipt.binding().operation.get()
    );
    assert_eq!(
        evidence["pending"][0]["publication_intent"]["receipt_complete"],
        true
    );
    assert_eq!(
        evidence["pending"][1]["publication_intent"]["receipt_complete"],
        true
    );
    assert!(first_path.join("consumer-generation.meta.hawdb").exists());
    assert!(second_path.join("consumer-generation.meta.hawdb").exists());
    drop(reopened);
}

fn candidate_metadata_identity(
    pending: &hawdb_storage::consumer_allocation::PendingConsumerGeneration,
) -> (
    hawdb_search::consumer::ConsumerBinding,
    NonZeroU64,
    NonZeroU64,
    NonZeroU64,
) {
    let original = pending.reservation_binding().unwrap();
    (
        hawdb_search::consumer::ConsumerBinding {
            database_uuid: original.database_uuid,
            projection_uuid: original.projection_uuid,
            consumer_id: original.consumer_id.to_owned(),
            registration_uuid: original.registration_uuid,
            checkpoint_uuid: original.checkpoint_uuid,
        },
        original.target_incarnation,
        original.generation,
        original.operation,
    )
}

fn candidate_metadata_limits() -> hawdb_search::consumer::generation_metadata::MetadataLimits {
    hawdb_search::consumer::generation_metadata::MetadataLimits {
        encoded_bytes: 64 << 10,
        artifacts: 128,
        artifact_bytes: 1 << 20,
    }
}

#[test]
fn consumer_native_generation_metadata_requires_the_original_reservation_identity() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let (binding, target_incarnation, generation, operation) =
        candidate_metadata_identity(&pending);
    let task = hawdb_core::RuntimeTaskContext::default();
    let lease = hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
        .unwrap();
    let mut input = hawdb_search::consumer::generation_metadata::MetadataInput {
        binding: &binding,
        source_epoch: 7,
        target_incarnation,
        generation,
        operation,
        component_uuid: hawdb_core::Uuid::from_bytes([5; 16]),
        search_manifest: candidate_reference(b"search head"),
        artifacts: &[],
    };
    lease.validate_generation_identity(&input).unwrap();
    input.operation = NonZeroU64::new(operation.get() + 1).unwrap();
    let error = lease.validate_generation_identity(&input).unwrap_err();
    assert!(error
        .to_string()
        .contains("differs from native reservation"));
    input.operation = operation;
    input.generation = NonZeroU64::new(generation.get() + 1).unwrap();
    assert!(lease.validate_generation_identity(&input).is_err());
}

#[test]
fn consumer_native_candidate_output_installs_a_bounded_durable_artifact() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("captured-candidate-input");
    std::fs::create_dir(&stage_path).unwrap();
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let payload = b"original candidate bytes";
    let mut staged = stage.create_child_new(&context, "payload").unwrap();
    std::io::Write::write_all(&mut staged, payload).unwrap();
    staged.sync_all().unwrap();
    stage.sync(&context).unwrap();
    drop(staged);

    let task = hawdb_core::RuntimeTaskContext::default();
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease.begin_candidate_output(payload.len() as u64).unwrap();
    let receipt = output
        .install_artifact(
            &stage,
            "payload",
            "search_projection_segment_payloads.0.hawdb",
            candidate_reference(payload),
        )
        .unwrap();
    assert_eq!(receipt.length, payload.len() as u64);
    assert_eq!(receipt.reference, candidate_reference(payload));
    assert_eq!(output.remaining_bytes(), 0);
    assert_eq!(
        std::fs::read(generation.join("search_projection_segment_payloads.0.hawdb")).unwrap(),
        payload
    );
    assert!(output
        .install_artifact(
            &stage,
            "payload",
            "over-budget",
            candidate_reference(payload)
        )
        .is_err());
    assert!(!generation.join("over-budget").exists());
    drop(output);
    assert!(lease.begin_candidate_output(payload.len() as u64).is_err());
    drop(lease);
    assert!(pending.born_directory().is_ok());
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[test]
fn consumer_native_candidate_output_rejects_a_foreign_descriptor_domain_before_creation() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let foreign_path = fixture.0.join("foreign-candidate-source");
    std::fs::create_dir(&foreign_path).unwrap();
    let foreign_project =
        hawdb_storage::file_descriptors::ProjectFileDescriptors::acquire(&foreign_path, 8).unwrap();
    let source = hawdb_storage::file_io::SourceDirectory::capture(
        &foreign_path,
        &foreign_project.io_context(),
    )
    .unwrap();
    std::fs::write(foreign_path.join("payload"), b"foreign").unwrap();
    let task = hawdb_core::RuntimeTaskContext::default();
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease.begin_candidate_output(7).unwrap();
    let error = output
        .install_artifact(
            &source,
            "payload",
            "foreign-output",
            candidate_reference(b"foreign"),
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("foreign descriptor domain"),
        "{error}"
    );
    assert_eq!(output.remaining_bytes(), 7);
    assert!(!generation.join("foreign-output").exists());
}

#[test]
fn consumer_native_candidate_output_rejects_changed_bytes_against_producer_reference() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("candidate-source-changed-in-place");
    std::fs::create_dir(&stage_path).unwrap();
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let original = b"original candidate bytes";
    let changed = b"modified candidate bytes";
    assert_eq!(original.len(), changed.len());
    std::fs::write(stage_path.join("payload"), original).unwrap();
    let expected = candidate_reference(original);
    std::fs::write(stage_path.join("payload"), changed).unwrap();

    let task = hawdb_core::RuntimeTaskContext::default();
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease.begin_candidate_output(original.len() as u64).unwrap();
    let unsupported = hawdb_storage::immutable_object::ObjectReference {
        format_version: 2,
        ..expected
    };
    let error = output
        .install_artifact(&stage, "payload", "unsupported", unsupported)
        .unwrap_err();
    assert!(
        error.to_string().contains("unsupported kind or version"),
        "{error}"
    );
    assert!(!generation.join("unsupported").exists());
    assert_eq!(output.remaining_bytes(), original.len() as u64);

    let error = output
        .install_artifact(&stage, "payload", "changed", expected)
        .unwrap_err();
    assert!(
        error.to_string().contains("differ from producer reference"),
        "{error}"
    );
    assert_eq!(output.remaining_bytes(), 0);
    assert!(generation.join("changed").exists());
    drop(output);
    drop(lease);
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[cfg(unix)]
#[test]
fn consumer_native_candidate_output_uses_the_original_stage_after_label_replacement() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("candidate-stage-label");
    let moved = fixture.0.join("original-candidate-stage");
    std::fs::create_dir(&stage_path).unwrap();
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let original = b"original candidate bytes";
    let foreign = b"replaced candidate bytes";
    assert_eq!(original.len(), foreign.len());
    let mut staged = stage.create_child_new(&context, "payload").unwrap();
    std::io::Write::write_all(&mut staged, original).unwrap();
    staged.sync_all().unwrap();
    drop(staged);
    std::fs::rename(&stage_path, &moved).unwrap();
    std::fs::create_dir(&stage_path).unwrap();
    std::fs::write(stage_path.join("payload"), foreign).unwrap();

    let task = hawdb_core::RuntimeTaskContext::default();
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease.begin_candidate_output(original.len() as u64).unwrap();
    output
        .install_artifact(&stage, "payload", "copied", candidate_reference(original))
        .unwrap();
    assert_eq!(std::fs::read(generation.join("copied")).unwrap(), original);
    assert_eq!(std::fs::read(stage_path.join("payload")).unwrap(), foreign);
}

#[test]
fn consumer_native_candidate_metadata_seals_verified_files_in_the_born_target() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };
    use hawdb_search::consumer::generation_metadata::{
        ArtifactReference, MetadataInput, MetadataView,
    };

    const MANIFEST: &str = "search_projection.out_of_core.manifest.hawdb";
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let target_path = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("candidate-metadata-stage");
    let mut producer = hawdb_search::SearchIndex::open(&stage_path).unwrap();
    producer
        .apply_projection_delta(hawdb_search::SearchProjectionDelta {
            upserts: vec![hawdb_search::SearchProjectionRow {
                kind: hawdb_search::SearchProjectionKind::Memory,
                external_id: "m1".into(),
                title: "candidate".into(),
                body: "immutable Search artifact".into(),
                embedding: None,
                source_id: None,
                metadata: Default::default(),
            }],
            source_graph_commit_epoch: Some(7),
            ..Default::default()
        })
        .unwrap();
    producer.checkpoint().unwrap();
    drop(producer);
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let manifest_bytes = std::fs::read(stage_path.join(MANIFEST)).unwrap();
    let mut artifact_files = std::fs::read_dir(&stage_path)
        .unwrap()
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().into_string().unwrap())
        .filter(|name| name == "search_projection.hawdb" || name.ends_with(".1.hawdb"))
        .collect::<Vec<_>>();
    artifact_files.sort();
    let artifact_bytes = artifact_files
        .iter()
        .map(|name| std::fs::read(stage_path.join(name)).unwrap())
        .collect::<Vec<_>>();

    let (binding, target_incarnation, generation, operation) =
        candidate_metadata_identity(&pending);
    let artifacts = artifact_files
        .iter()
        .zip(&artifact_bytes)
        .map(|(name, bytes)| ArtifactReference {
            name,
            reference: candidate_reference(bytes),
        })
        .collect::<Vec<_>>();
    let input = MetadataInput {
        binding: &binding,
        source_epoch: 7,
        target_incarnation,
        generation,
        operation,
        component_uuid: hawdb_core::Uuid::from_bytes([5; 16]),
        search_manifest: candidate_reference(&manifest_bytes),
        artifacts: &artifacts,
    };
    let limits = candidate_metadata_limits();
    let task = hawdb_core::RuntimeTaskContext::default();
    let metadata_len = input.encoded_len(limits, &task).unwrap() as u64;
    let total = manifest_bytes.len() as u64
        + artifact_bytes
            .iter()
            .map(|bytes| bytes.len() as u64)
            .sum::<u64>()
        + metadata_len;
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    let permit = std::sync::Arc::new(
        governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024).with_io_wave_slots(1),
            )
            .unwrap(),
    );
    let mut lease = hawdb_search::consumer::ConsumerStationaryTargetLease::acquire_governed(
        &mut pending,
        permit,
        task.clone(),
    )
    .unwrap();
    let mut output = lease.begin_candidate_output(total).unwrap();
    output
        .install_artifact(&stage, MANIFEST, MANIFEST, input.search_manifest)
        .unwrap();
    for artifact in &artifacts {
        output
            .install_artifact(&stage, artifact.name, artifact.name, artifact.reference)
            .unwrap();
    }
    let receipt = output.seal_generation_metadata(&input, limits).unwrap();
    let bytes = std::fs::read(target_path.join("consumer-generation.meta.hawdb")).unwrap();
    assert_eq!(receipt.encoded_len, metadata_len);
    assert_eq!(receipt.reference, candidate_reference(&bytes));
    assert_eq!(output.remaining_bytes(), 0);
    let view = MetadataView::decode(&bytes, limits, &task).unwrap();
    assert!(view.matches_binding(&binding));
    assert_eq!(view.operation(), operation);
    assert_eq!(view.search_manifest(), input.search_manifest);
    for artifact in &artifacts {
        assert_eq!(view.artifact(artifact.name), Some(artifact.reference));
    }
    assert!(output
        .install_artifact(
            &stage,
            artifacts[0].name,
            "after-seal",
            artifacts[0].reference
        )
        .is_err());
    assert!(output.seal_generation_metadata(&input, limits).is_err());
    drop(output);

    #[cfg(unix)]
    {
        let original = target_path.with_extension("original");
        std::fs::rename(&target_path, &original).unwrap();
        std::fs::create_dir(&target_path).unwrap();
        std::fs::write(target_path.join(MANIFEST), b"foreign manifest").unwrap();
    }
    let view = lease
        .prepare_candidate_view(std::num::NonZeroU64::new(32 * 1024 * 1024).unwrap())
        .unwrap();
    assert_eq!(view.source_epoch, 7);
    assert_eq!(view.document_count, 1);
    assert_eq!(view.metadata_reference, receipt.reference);
    assert!(lease
        .prepare_candidate_view(std::num::NonZeroU64::new(32 * 1024 * 1024).unwrap())
        .is_err());
}

fn sealed_native_candidate_lease<'birth>(
    pending: &'birth mut hawdb_storage::consumer_allocation::PendingConsumerGeneration,
    fixture: &Fixture,
    source_epoch: u64,
    governor: &hawdb_qos::RuntimeGovernor,
    content: &str,
) -> hawdb_search::consumer::ConsumerStationaryTargetLease<'birth> {
    sealed_native_candidate_document_lease(
        pending,
        fixture,
        source_epoch,
        governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: content.into(),
            embedding: None,
            metadata: Default::default(),
        },
    )
}

fn sealed_native_candidate_document_lease<'birth>(
    pending: &'birth mut hawdb_storage::consumer_allocation::PendingConsumerGeneration,
    fixture: &Fixture,
    source_epoch: u64,
    governor: &hawdb_qos::RuntimeGovernor,
    document: hawdb_search::SearchDocument,
) -> hawdb_search::consumer::ConsumerStationaryTargetLease<'birth> {
    sealed_native_candidate_delta_lease(pending, fixture, source_epoch, governor, document, None)
}

fn sealed_native_candidate_delta_lease<'birth>(
    pending: &'birth mut hawdb_storage::consumer_allocation::PendingConsumerGeneration,
    fixture: &Fixture,
    source_epoch: u64,
    governor: &hawdb_qos::RuntimeGovernor,
    document: hawdb_search::SearchDocument,
    delta: Option<hawdb_search::SearchProjectionDelta>,
) -> hawdb_search::consumer::ConsumerStationaryTargetLease<'birth> {
    use hawdb_qos::RuntimeWorkRequest;
    use hawdb_search::consumer::generation_metadata::{ArtifactReference, MetadataInput};
    const MANIFEST: &str = "search_projection.out_of_core.manifest.hawdb";
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture
        .0
        .join(format!("{}.producer", pending.generation_name()));
    let mut writer = hawdb_search::SearchOutOfCoreGenerationWriter::create(
        &stage_path,
        hawdb_search::SearchOutOfCoreGenerationBuildOptions {
            source_graph_commit_epoch: Some(if delta.is_some() {
                source_epoch - 1
            } else {
                source_epoch
            }),
            ..Default::default()
        },
    )
    .unwrap();
    writer.push(document).unwrap();
    writer.finish().unwrap();
    if let Some(delta) = delta {
        let reader = hawdb_search::SearchOutOfCoreReader::open(&stage_path).unwrap();
        let update = hawdb_search::SearchOutOfCoreGenerationWriter::prepare_delta(
            &reader,
            delta,
            Default::default(),
        )
        .unwrap();
        drop(reader);
        let (_, report, _) = update.finish().unwrap();
        assert_eq!(report.generation, 2);
    }
    assert!(!stage_path.join("search_projection.hawdb").exists());
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let manifest_bytes = std::fs::read(stage_path.join(MANIFEST)).unwrap();
    let mut names = std::fs::read_dir(&stage_path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| {
            name.strip_suffix(".hawdb")
                .and_then(|stem| stem.rsplit('.').next())
                .is_some_and(|generation| generation.parse::<u64>().is_ok())
        })
        .collect::<Vec<_>>();
    names.sort();
    let bytes = names
        .iter()
        .map(|name| std::fs::read(stage_path.join(name)).unwrap())
        .collect::<Vec<_>>();
    let artifacts = names
        .iter()
        .zip(&bytes)
        .map(|(name, bytes)| ArtifactReference {
            name,
            reference: candidate_reference(bytes),
        })
        .collect::<Vec<_>>();
    let (binding, target_incarnation, generation, operation) = candidate_metadata_identity(pending);
    let input = MetadataInput {
        binding: &binding,
        source_epoch,
        target_incarnation,
        generation,
        operation,
        component_uuid: hawdb_core::Uuid::from_bytes([5; 16]),
        search_manifest: candidate_reference(&manifest_bytes),
        artifacts: &artifacts,
    };
    let limits = candidate_metadata_limits();
    let task = hawdb_core::RuntimeTaskContext::default();
    let total = manifest_bytes.len() as u64
        + bytes.iter().map(|bytes| bytes.len() as u64).sum::<u64>()
        + input.encoded_len(limits, &task).unwrap() as u64;
    let permit = std::sync::Arc::new(
        governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                    .with_result_bytes(1024 * 1024)
                    .with_io_wave_slots(1),
            )
            .unwrap(),
    );
    let mut lease = hawdb_search::consumer::ConsumerStationaryTargetLease::acquire_governed(
        pending, permit, task,
    )
    .unwrap();
    let mut output = lease.begin_candidate_output(total).unwrap();
    output
        .install_artifact(&stage, MANIFEST, MANIFEST, input.search_manifest)
        .unwrap();
    for artifact in &artifacts {
        output
            .install_artifact(&stage, artifact.name, artifact.name, artifact.reference)
            .unwrap();
    }
    output.seal_generation_metadata(&input, limits).unwrap();
    drop(output);
    lease
}

#[test]
fn consumer_native_candidate_view_selects_durably_without_resident_snapshot() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };

    const MANIFEST: &str = "search_projection.out_of_core.manifest.hawdb";
    #[derive(Clone, Copy)]
    enum SelectionCase {
        Complete,
        LostResponse,
        Stale,
        SourceConflict,
        #[cfg(unix)]
        NativeFailure,
    }
    for (tamper_after_seal, selection_case) in [
        (false, SelectionCase::Complete),
        (false, SelectionCase::LostResponse),
        (false, SelectionCase::Stale),
        (false, SelectionCase::SourceConflict),
        #[cfg(unix)]
        (false, SelectionCase::NativeFailure),
        (true, SelectionCase::Complete),
    ] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let allocation = reserve_native_generation(&mut db, &fixture);
        let source_limits = crate::PreparedReadLimits {
            dependency_entries: std::num::NonZeroUsize::new(8).unwrap(),
            dependency_bytes: std::num::NonZeroUsize::new(4096).unwrap(),
            statement_rows: std::num::NonZeroUsize::new(8).unwrap(),
            statement_payload_bytes: std::num::NonZeroUsize::new(4096).unwrap(),
        };
        let mut source_read = db.begin_prepared_read(source_limits).unwrap();
        source_read
            .query_with_params("MATCH (n:Memory) RETURN n.content", &Default::default())
            .unwrap();
        let source = source_read.finish().unwrap();
        let source_epoch = source.source_epoch();
        let mut pending = allocation
            .prepare_stationary_generation(&fixture.0)
            .unwrap();
        pending.create().unwrap();
        let target_path = fixture.0.join(pending.generation_name());
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(64 * 1024 * 1024),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        let operation = pending.reservation_binding().unwrap().operation;
        let mut lease = sealed_native_candidate_lease(
            &mut pending,
            &fixture,
            source_epoch,
            &governor,
            "out of core search document",
        );
        assert!(!target_path.join("search_projection.hawdb").exists());
        #[cfg(feature = "full-text-search")]
        assert!(lease
            .probe_candidate_query(
                "out of core",
                None,
                hawdb_search::SearchMode::Text,
                hawdb_search::SearchQueryOptions {
                    limit: 1,
                    offset: 0,
                    rank_window: None,
                    fusion_weights: Default::default(),
                    metadata_filters: Default::default(),
                    policy_epoch: None,
                },
            )
            .is_err());

        #[cfg(unix)]
        let captured_path = {
            let original = target_path.with_extension("original");
            std::fs::rename(&target_path, &original).unwrap();
            std::fs::create_dir(&target_path).unwrap();
            std::fs::write(target_path.join(MANIFEST), b"foreign manifest").unwrap();
            original
        };
        #[cfg(not(unix))]
        let captured_path = target_path;
        if tamper_after_seal {
            std::fs::write(captured_path.join(MANIFEST), b"changed after seal").unwrap();
            let error = lease
                .prepare_candidate_view(std::num::NonZeroU64::new(32 * 1024 * 1024).unwrap())
                .unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("differs from producer reference"),
                "{error}"
            );
        } else {
            let view = lease
                .prepare_candidate_view(std::num::NonZeroU64::new(32 * 1024 * 1024).unwrap())
                .unwrap();
            assert_eq!(view.source_epoch, source_epoch);
            assert_eq!(view.document_count, 1);
            #[cfg(feature = "full-text-search")]
            {
                let result = lease
                    .probe_candidate_query(
                        "out of core",
                        None,
                        hawdb_search::SearchMode::Text,
                        hawdb_search::SearchQueryOptions {
                            limit: 1,
                            offset: 0,
                            rank_window: None,
                            fusion_weights: Default::default(),
                            metadata_filters: Default::default(),
                            policy_epoch: None,
                        },
                    )
                    .unwrap();
                assert_eq!(result.result().total_hits, 1);
                assert_eq!(result.result().hits[0].id, "memory:m1");
                assert!(
                    !result.result().hits[0]
                        .projection_freshness
                        .full_reindex_needed
                );
                assert!(
                    !result.result().hits[0]
                        .projection_freshness
                        .metadata_repair_needed
                );
                assert_eq!(
                    result.result().hits[0]
                        .projection_freshness
                        .source_graph_commit_epoch,
                    Some(source_epoch)
                );
            }
            let detached = lease.detach_candidate().unwrap();
            assert_eq!(governor.snapshot().active_background_tasks, 0);
            let mut wrong_birth = reserve_native_generation(&mut db, &fixture)
                .prepare_stationary_generation(&fixture.0)
                .unwrap();
            wrong_birth.create().unwrap();
            let (error, detached, wrong_birth) =
                (*detached.attach_birth(wrong_birth).unwrap_err()).into_parts();
            assert!(error
                .to_string()
                .contains("differs from its recorded native birth"));
            assert!(wrong_birth.born_directory().is_ok());
            let mut prepared = detached.attach_birth(pending).unwrap();
            assert_eq!(prepared.report(), view);
            let error = db
                .validate_prepared_consumer_candidate_source(
                    &source,
                    &prepared,
                    crate::ProjectionSourceDurability::Synchronize,
                )
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("candidate seal differs from durable reservation"));
            prepared.record_durable_seal().unwrap();
            prepared.record_durable_seal().unwrap();
            let evidence = allocation_evidence(&fixture).1;
            assert_eq!(
                evidence["pending"][0]["candidate_seal"]["source_epoch"],
                source_epoch
            );
            assert_eq!(
                evidence["pending"][0]["candidate_seal"]["metadata_sha256"],
                view.metadata_reference.sha256.to_string()
            );
            db.validate_prepared_consumer_candidate_source(
                &source,
                &prepared,
                crate::ProjectionSourceDurability::Synchronize,
            )
            .unwrap();
            prepared.record_publication_intent().unwrap();
            prepared.record_publication_intent().unwrap();
            assert!(
                allocation_evidence(&fixture).1["pending"][0]["publication_intent"]["previous"]
                    .is_null()
            );
            let preparation_permit = governor
                .try_admit(
                    RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                        .with_result_bytes(1024 * 1024)
                        .with_io_wave_slots(1),
                )
                .unwrap();
            prepared
                .prepare_selection(
                    &preparation_permit,
                    hawdb_core::RuntimeTaskContext::default(),
                )
                .unwrap();
            let read_slot = prepared.create_read_slot(&preparation_permit).unwrap();
            assert!(matches!(
                read_slot.report(),
                Err(HawDBError::ProjectionBusy)
            ));
            prepared
                .bind_read_slot(&read_slot, &preparation_permit)
                .unwrap();
            drop(preparation_permit);
            assert_eq!(governor.snapshot().active_background_tasks, 0);
            if matches!(selection_case, SelectionCase::Stale) {
                let unrelated = reserve_native_generation(&mut db, &fixture);
                drop(unrelated);
            }
            if matches!(selection_case, SelectionCase::SourceConflict) {
                db.query("MATCH (n:Memory) SET n.content = 'source changed'")
                    .unwrap();
            }
            #[cfg(unix)]
            if matches!(selection_case, SelectionCase::NativeFailure) {
                let selector = std::fs::read_dir(fixture.0.join("database"))
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .unwrap()
                            .to_str()
                            .unwrap()
                            .starts_with("consumer-selector-")
                    })
                    .unwrap();
                std::fs::remove_file(selector).unwrap();
            }
            let selection_permit = std::sync::Arc::new(
                governor
                    .try_admit(
                        RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                            .with_result_bytes(1024 * 1024)
                            .with_io_wave_slots(1),
                    )
                    .unwrap(),
            );
            let mut attempt = prepared
                .admit_selection(selection_permit, hawdb_core::RuntimeTaskContext::default())
                .unwrap();
            match selection_case {
                SelectionCase::Complete => {
                    let pending = db
                        .select_prepared_consumer_candidate(&source, &mut attempt)
                        .unwrap();
                    assert!(matches!(
                        read_slot.report(),
                        Err(HawDBError::ProjectionBusy)
                    ));
                    append(&mut db, 2);
                    let receipt = pending.finish().unwrap();
                    assert_eq!(receipt.source_epoch(), source_epoch);
                    assert_eq!(receipt.metadata_reference(), view.metadata_reference);
                    assert_eq!(receipt.binding().operation, operation);
                }
                SelectionCase::LostResponse => {
                    let pending = db
                        .select_prepared_consumer_candidate(&source, &mut attempt)
                        .unwrap();
                    drop(pending);
                    drop(attempt);
                    assert!(matches!(
                        read_slot.report(),
                        Err(HawDBError::ProjectionBusy)
                    ));
                    assert_eq!(governor.snapshot().active_background_tasks, 0);
                    append(&mut db, 2);
                    let retry_permit = std::sync::Arc::new(
                        governor
                            .try_admit(
                                RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                                    .with_result_bytes(1024 * 1024)
                                    .with_io_wave_slots(1),
                            )
                            .unwrap(),
                    );
                    attempt = prepared
                        .admit_selection(retry_permit, hawdb_core::RuntimeTaskContext::default())
                        .unwrap();
                    let hawdb_storage::consumer_allocation::ConsumerSelectionResolution::Published(
                        receipt,
                    ) = attempt.resolve().unwrap()
                    else {
                        panic!("lost selection response lost its committed possibility");
                    };
                    assert_eq!(receipt.source_epoch(), source_epoch);
                    assert_eq!(receipt.metadata_reference(), view.metadata_reference);
                }
                SelectionCase::Stale => {
                    let error = db
                        .select_prepared_consumer_candidate(&source, &mut attempt)
                        .unwrap_err();
                    assert!(error.to_string().contains("preparation became stale"));
                    assert_eq!(
                        attempt.resolve().unwrap(),
                        hawdb_storage::consumer_allocation::ConsumerSelectionResolution::NotSelected,
                    );
                    assert_eq!(
                        allocation_evidence(&fixture).1["pending"]
                            .as_array()
                            .unwrap()
                            .len(),
                        3
                    );
                    append(&mut db, 2);
                }
                SelectionCase::SourceConflict => {
                    let error = db
                        .select_prepared_consumer_candidate(&source, &mut attempt)
                        .unwrap_err();
                    assert!(matches!(
                        error,
                        crate::ProjectionPublicationError::Dependency(_)
                    ));
                    attempt.cancel_unattempted().unwrap();
                    assert_eq!(
                        allocation_evidence(&fixture).1["pending"][0]["publication_intent"]
                            ["not_selected"],
                        true
                    );
                    append(&mut db, 2);
                }
                #[cfg(unix)]
                SelectionCase::NativeFailure => {
                    assert!(db
                        .select_prepared_consumer_candidate(&source, &mut attempt)
                        .is_err());
                    assert_eq!(
                        attempt.resolve().unwrap(),
                        hawdb_storage::consumer_allocation::ConsumerSelectionResolution::NotSelected,
                    );
                    assert_eq!(
                        allocation_evidence(&fixture).1["pending"][0]["publication_intent"]
                            ["not_selected"],
                        true
                    );
                    append(&mut db, 2);
                }
            }
            drop(attempt);
            assert_eq!(governor.snapshot().active_background_tasks, 0);
            let evidence = allocation_evidence(&fixture).1;
            let selected = match selection_case {
                SelectionCase::Complete | SelectionCase::LostResponse => true,
                SelectionCase::Stale | SelectionCase::SourceConflict => false,
                #[cfg(unix)]
                SelectionCase::NativeFailure => false,
            };
            if !selected {
                assert!(evidence.get("selected").is_none());
                assert!(matches!(
                    read_slot.report(),
                    Err(HawDBError::ProjectionBusy)
                ));
            } else {
                assert_eq!(read_slot.report().unwrap(), view);
                assert_eq!(evidence["selected"][0]["operation"], operation.get());
                assert_eq!(
                    evidence["pending"][0]["publication_intent"]["receipt_complete"],
                    true
                );
            }
            let foreign = Fixture::new();
            let mut foreign_db = foreign.database();
            assert!(foreign_db
                .validate_prepared_consumer_candidate_source(
                    &source,
                    &prepared,
                    crate::ProjectionSourceDurability::Synchronize,
                )
                .is_err());
            #[cfg(feature = "full-text-search")]
            {
                let foreign_governor = RuntimeGovernor::detect(
                    RuntimeGovernorConfig {
                        memory_budget_bytes: Some(64 * 1024 * 1024),
                        ..RuntimeGovernorConfig::shared_host()
                    },
                    IoConcurrencyBudget::new(2, 1),
                );
                let foreign_permit = foreign_governor
                    .try_admit(
                        RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                            .with_result_bytes(1024 * 1024)
                            .with_io_wave_slots(1),
                    )
                    .unwrap();
                assert!(read_slot
                    .pin(&foreign_permit)
                    .unwrap_err()
                    .to_string()
                    .contains("another governor"));
                assert!(prepared
                    .probe_candidate_query(
                        &foreign_permit,
                        hawdb_core::RuntimeTaskContext::default(),
                        "out of core",
                        None,
                        hawdb_search::SearchMode::Text,
                        hawdb_search::SearchQueryOptions {
                            limit: 1,
                            offset: 0,
                            rank_window: None,
                            fusion_weights: Default::default(),
                            metadata_filters: Default::default(),
                            policy_epoch: None,
                        },
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("another governor"));
                let query_permit = governor
                    .try_admit(
                        RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                            .with_result_bytes(1024 * 1024)
                            .with_io_wave_slots(1),
                    )
                    .unwrap();
                assert_eq!(
                    prepared
                        .probe_candidate_query(
                            &query_permit,
                            hawdb_core::RuntimeTaskContext::default(),
                            "out of core",
                            None,
                            hawdb_search::SearchMode::Text,
                            hawdb_search::SearchQueryOptions {
                                limit: 1,
                                offset: 0,
                                rank_window: None,
                                fusion_weights: Default::default(),
                                metadata_filters: Default::default(),
                                policy_epoch: None,
                            },
                        )
                        .unwrap()
                        .result()
                        .hits[0]
                        .id,
                    "memory:m1"
                );
            }
            let later = db
                .begin_prepared_read(source_limits)
                .unwrap()
                .finish()
                .unwrap();
            let error = db
                .validate_prepared_consumer_candidate_source(
                    &later,
                    &prepared,
                    crate::ProjectionSourceDurability::Synchronize,
                )
                .unwrap_err();
            assert!(error
                .to_string()
                .contains("does not match captured source epoch"));
            let original_birth = prepared.abandon();
            assert!(original_birth.born_directory().is_ok());
            drop(original_birth);
            drop(wrong_birth);
            drop(db);
            let reopened = fixture.database();
            assert_eq!(
                allocation_evidence(&fixture).1["pending"][0]["candidate_seal"]["source_epoch"],
                source_epoch
            );
            assert!(
                allocation_evidence(&fixture).1["pending"][0]["publication_intent"]["previous"]
                    .is_null()
            );
            if selected {
                let evidence = allocation_evidence(&fixture).1;
                assert_eq!(
                    evidence["selected"][0]["seal"]["source_epoch"],
                    source_epoch
                );
                assert_eq!(
                    evidence["pending"][0]["publication_intent"]["receipt_complete"],
                    true
                );
            }
            drop(reopened);
        }
    }
}

#[test]
fn consumer_native_candidate_view_keeps_old_pins_across_successor_and_failed_selection() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };
    use hawdb_search::consumer::ConsumerPreparedCandidate;
    use hawdb_storage::consumer_allocation::ConsumerSelectionResolution;
    use std::num::NonZeroUsize;

    fn candidate(
        db: &mut Database,
        fixture: &Fixture,
        governor: &RuntimeGovernor,
        content: &str,
    ) -> (ConsumerPreparedCandidate, crate::PreparedReadDependencies) {
        let allocation = reserve_native_generation(db, fixture);
        let mut read = db
            .begin_prepared_read(crate::PreparedReadLimits {
                dependency_entries: NonZeroUsize::new(8).unwrap(),
                dependency_bytes: NonZeroUsize::new(4096).unwrap(),
                statement_rows: NonZeroUsize::new(8).unwrap(),
                statement_payload_bytes: NonZeroUsize::new(4096).unwrap(),
            })
            .unwrap();
        read.query_with_params("MATCH (n:Memory) RETURN n.content", &Default::default())
            .unwrap();
        let source = read.finish().unwrap();
        let mut birth = allocation
            .prepare_stationary_generation(&fixture.0)
            .unwrap();
        birth.create().unwrap();
        let mut lease = sealed_native_candidate_lease(
            &mut birth,
            fixture,
            source.source_epoch(),
            governor,
            content,
        );
        lease
            .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
            .unwrap();
        let detached = lease.detach_candidate().unwrap();
        let mut candidate = detached.attach_birth(birth).unwrap();
        candidate.record_durable_seal().unwrap();
        db.validate_prepared_consumer_candidate_source(
            &source,
            &candidate,
            crate::ProjectionSourceDurability::Synchronize,
        )
        .unwrap();
        candidate.record_publication_intent().unwrap();
        let permit = admit(governor);
        candidate
            .prepare_selection(&permit, Default::default())
            .unwrap();
        (candidate, source)
    }

    fn admit(governor: &RuntimeGovernor) -> std::sync::Arc<hawdb_qos::RuntimePermit> {
        std::sync::Arc::new(
            governor
                .try_admit(
                    RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                        .with_result_bytes(1024 * 1024)
                        .with_io_wave_slots(1),
                )
                .unwrap(),
        )
    }

    #[cfg(feature = "full-text-search")]
    fn hits(
        pin: &hawdb_search::consumer::ConsumerReadPin,
        governor: &RuntimeGovernor,
        term: &str,
    ) -> usize {
        pin.probe_query(
            &admit(governor),
            Default::default(),
            term,
            None,
            hawdb_search::SearchMode::Text,
            hawdb_search::SearchQueryOptions {
                limit: 1,
                offset: 0,
                rank_window: None,
                fusion_weights: Default::default(),
                metadata_filters: Default::default(),
                policy_epoch: None,
            },
        )
        .unwrap()
        .result()
        .total_hits
    }

    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    let (mut first, source) = candidate(&mut db, &fixture, &governor, "amber");
    let permit = admit(&governor);
    let slot = first.create_read_slot(&permit).unwrap();
    first.bind_read_slot(&slot, &permit).unwrap();
    let mut attempt = first
        .admit_selection(permit.clone(), Default::default())
        .unwrap();
    let receipt = db
        .select_prepared_consumer_candidate(&source, &mut attempt)
        .unwrap()
        .finish()
        .unwrap();
    drop(attempt);
    let old = slot.pin(&permit).unwrap();
    assert_eq!(old.report(), first.report());
    drop(permit);
    assert_eq!(governor.snapshot().active_background_tasks, 0);

    #[cfg(feature = "full-text-search")]
    {
        let before = governor.snapshot().admitted_memory_bytes;
        let tiny = governor
            .try_admit(RuntimeWorkRequest::background_maintenance(1024).with_io_wave_slots(0))
            .unwrap();
        let error = old
            .probe_query(
                &tiny,
                Default::default(),
                "amber",
                None,
                hawdb_search::SearchMode::Text,
                hawdb_search::SearchQueryOptions {
                    limit: 1,
                    offset: 0,
                    rank_window: None,
                    fusion_weights: Default::default(),
                    metadata_filters: Default::default(),
                    policy_epoch: None,
                },
            )
            .unwrap_err();
        assert!(
            matches!(
                error,
                crate::SearchQueryError::Query(HawDBError::ResourceBudgetExceeded(_))
            ),
            "{error}"
        );
        drop(tiny);
        assert_eq!(slot.report().unwrap(), old.report());
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().admitted_memory_bytes, before);
        assert_eq!(hits(&old, &governor, "amber"), 1);
    }

    append(&mut db, 2);
    let (mut second, source) = candidate(&mut db, &fixture, &governor, "violet");
    let permit = admit(&governor);
    let empty = second.create_read_slot(&permit).unwrap();
    assert!(second.bind_read_slot(&empty, &permit).is_err());
    drop(empty);
    second.bind_read_slot(&slot, &permit).unwrap();
    let mut attempt = second
        .admit_selection(permit.clone(), Default::default())
        .unwrap();
    let pending = db
        .select_prepared_consumer_candidate(&source, &mut attempt)
        .unwrap();
    assert!(matches!(slot.report(), Err(HawDBError::ProjectionBusy)));
    assert!(matches!(slot.pin(&permit), Err(HawDBError::ProjectionBusy)));
    drop(pending);
    drop(attempt);
    drop(permit);
    // The source can advance and old readers continue while the lost response
    // keeps new reader admission closed.
    append(&mut db, 3);
    #[cfg(feature = "full-text-search")]
    std::thread::scope(|scope| {
        assert_eq!(
            scope
                .spawn(|| hits(&old, &governor, "amber"))
                .join()
                .unwrap(),
            1
        );
    });
    let permit = admit(&governor);
    let mut attempt = second
        .admit_selection(permit.clone(), Default::default())
        .unwrap();
    assert!(matches!(
        attempt.resolve().unwrap(),
        ConsumerSelectionResolution::Published(_)
    ));
    drop(attempt);
    let new = slot.pin(&permit).unwrap();
    assert_eq!(new.report(), second.report());
    assert!(new.report().source_epoch > old.report().source_epoch);
    // Repeating an old receipt must not revert the successor's runtime slot.
    let mut historical = first
        .admit_selection(permit.clone(), Default::default())
        .unwrap();
    assert_eq!(
        historical.resolve().unwrap(),
        ConsumerSelectionResolution::Published(Box::new(receipt))
    );
    drop(historical);
    assert_eq!(slot.report().unwrap(), new.report());
    drop(permit);

    let (mut rejected, source) = candidate(&mut db, &fixture, &governor, "discarded");
    let permit = admit(&governor);
    rejected.bind_read_slot(&slot, &permit).unwrap();
    drop(reserve_native_generation(&mut db, &fixture));
    let mut attempt = rejected
        .admit_selection(permit.clone(), Default::default())
        .unwrap();
    assert!(db
        .select_prepared_consumer_candidate(&source, &mut attempt)
        .is_err());
    assert!(matches!(slot.report(), Err(HawDBError::ProjectionBusy)));
    assert_eq!(
        attempt.resolve().unwrap(),
        ConsumerSelectionResolution::NotSelected
    );
    drop(attempt);
    assert_eq!(slot.report().unwrap(), new.report());
    drop(permit);
    drop(rejected.abandon());
    drop(first.abandon());
    drop(second.abandon());
    drop(slot);
    drop(db);
    let reopened = fixture.database();
    #[cfg(feature = "full-text-search")]
    {
        assert_eq!(hits(&old, &governor, "amber"), 1);
        assert_eq!(hits(&old, &governor, "violet"), 0);
        assert_eq!(hits(&new, &governor, "violet"), 1);
        assert_eq!(hits(&new, &governor, "amber"), 0);
    }
    drop(old);
    drop(new);
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    drop(reopened);
}

#[test]
#[cfg(feature = "full-text-search")]
fn consumer_native_candidate_view_queries_incremental_retractions_with_fresh_admission() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };
    for deleted in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        append(&mut db, 2);
        let mut birth = reserve_native_generation(&mut db, &fixture)
            .prepare_stationary_generation(&fixture.0)
            .unwrap();
        birth.create().unwrap();
        #[cfg(unix)]
        let target_path = fixture.0.join(birth.generation_name());
        let epoch = db.begin_read_transaction().unwrap().commit_epoch();
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(64 * 1024 * 1024),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        let initial = hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: "amber".into(),
            embedding: None,
            metadata: std::collections::BTreeMap::from([("space_id".into(), "team".into())]),
        };
        let row = hawdb_search::SearchProjectionRow {
            kind: hawdb_search::SearchProjectionKind::Memory,
            external_id: "m1".into(),
            title: "candidate".into(),
            body: "violet".into(),
            embedding: None,
            source_id: None,
            metadata: std::collections::BTreeMap::from([("space_id".into(), "private".into())]),
        };
        let delta = hawdb_search::SearchProjectionDelta {
            upserts: if deleted { Vec::new() } else { vec![row] },
            deletes: if deleted {
                vec!["memory:m1".into()]
            } else {
                Vec::new()
            },
            max_operations: Some(8),
            source_graph_commit_epoch: Some(epoch),
        };
        let mut lease = sealed_native_candidate_delta_lease(
            &mut birth,
            &fixture,
            epoch,
            &governor,
            initial,
            Some(delta),
        );
        #[cfg(unix)]
        {
            // Incremental target validation must create its scratch through the
            // captured native parent, even if the diagnostic name is replaced.
            std::fs::rename(&target_path, target_path.with_extension("original")).unwrap();
            std::fs::create_dir(&target_path).unwrap();
            std::fs::write(target_path.join("foreign"), b"untouched").unwrap();
        }
        let report = lease
            .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
            .unwrap();
        assert_eq!(report.generation, 2);
        assert_eq!(report.document_count, usize::from(!deleted));
        let detached = lease.detach_candidate().unwrap();
        let candidate = detached.attach_birth(birth).unwrap();
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        for (term, space, expected) in [
            ("amber", "team", 0),
            ("violet", "team", 0),
            ("violet", "private", usize::from(!deleted)),
        ] {
            let permit = governor
                .try_admit(
                    RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                        .with_result_bytes(1024 * 1024)
                        .with_io_wave_slots(1),
                )
                .unwrap();
            let output = candidate
                .probe_candidate_query(
                    &permit,
                    hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(
                        10,
                    )),
                    term,
                    None,
                    hawdb_search::SearchMode::Text,
                    hawdb_search::SearchQueryOptions {
                        limit: 4,
                        offset: 0,
                        rank_window: None,
                        fusion_weights: Default::default(),
                        metadata_filters: std::collections::BTreeMap::from([(
                            "space_id".into(),
                            space.into(),
                        )]),
                        policy_epoch: None,
                    },
                )
                .unwrap();
            assert_eq!(output.result().total_hits, expected);
            if expected != 0 {
                assert_eq!(output.result().hits[0].id, "memory:m1");
            }
            drop(permit);
            assert_eq!(governor.snapshot().active_background_tasks, 0);
            assert_eq!(governor.snapshot().active_background_io_slots, 0);
        }
        drop(candidate.abandon());
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
        #[cfg(unix)]
        {
            assert_eq!(
                std::fs::read(target_path.join("foreign")).unwrap(),
                b"untouched"
            );
            assert_eq!(std::fs::read_dir(target_path).unwrap().count(), 1);
        }
    }
}

#[test]
#[cfg(feature = "vector-search")]
fn consumer_native_candidate_view_probes_real_vector_and_hybrid_with_one_io_slot() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut birth = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    birth.create().unwrap();
    let epoch = db.begin_read_transaction().unwrap().commit_epoch();
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    let mut lease = sealed_native_candidate_document_lease(
        &mut birth,
        &fixture,
        epoch,
        &governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: "amber".into(),
            embedding: Some(vec![1.0, 0.0]),
            metadata: std::collections::BTreeMap::from([("space_id".into(), "team".into())]),
        },
    );
    lease
        .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
        .unwrap();
    let detached = lease.detach_candidate().unwrap();
    let candidate = detached.attach_birth(birth).unwrap();
    for mode in [
        hawdb_search::SearchMode::Vector,
        #[cfg(feature = "full-text-search")]
        hawdb_search::SearchMode::Hybrid,
        #[cfg(feature = "full-text-search")]
        hawdb_search::SearchMode::Text,
    ] {
        let permit = governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                    .with_result_bytes(1024 * 1024)
                    .with_io_wave_slots(1),
            )
            .unwrap();
        let output = candidate
            .probe_candidate_query(
                &permit,
                hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(10)),
                "amber",
                Some(&[1.0, 0.0]),
                mode,
                hawdb_search::SearchQueryOptions {
                    limit: 1,
                    offset: 0,
                    rank_window: None,
                    fusion_weights: Default::default(),
                    metadata_filters: std::collections::BTreeMap::from([(
                        "space_id".into(),
                        "team".into(),
                    )]),
                    policy_epoch: None,
                },
            )
            .unwrap();
        assert_eq!(output.result().total_hits, 1);
        assert_eq!(output.result().hits[0].id, "memory:m1");
        assert!(
            !output.result().hits[0]
                .projection_freshness
                .full_reindex_needed
        );
        assert!(
            !output.result().hits[0]
                .projection_freshness
                .metadata_repair_needed
        );
        drop(permit);
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
    }
    // With no metadata filters or text work, the tiny refusal comes from the
    // vector read/decode path, rather than an earlier candidate-filter stage.
    let before = governor.snapshot().admitted_memory_bytes;
    for budget in [64 * 1024, 3 * 1024 * 1024, 16 * 1024 * 1024] {
        let permit = governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(budget)
                    .with_result_bytes(1024 * 1024)
                    .with_io_wave_slots(1),
            )
            .unwrap();
        let result = candidate.probe_candidate_query(
            &permit,
            hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(10)),
            "",
            Some(&[1.0, 0.0]),
            hawdb_search::SearchMode::Vector,
            hawdb_search::SearchQueryOptions {
                limit: 1,
                offset: 0,
                rank_window: None,
                fusion_weights: Default::default(),
                metadata_filters: Default::default(),
                policy_epoch: None,
            },
        );
        if budget == 64 * 1024 {
            assert!(
                matches!(
                    result,
                    Err(crate::SearchQueryError::Query(
                        HawDBError::ResourceBudgetExceeded(_)
                    ))
                ),
                "{result:?}"
            );
        } else {
            assert_eq!(result.unwrap().result().total_hits, 1);
        }
        drop(permit);
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
        assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    }
    drop(candidate.abandon());
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
#[cfg(feature = "vector-search")]
fn consumer_native_candidate_view_hydration_refuses_then_retries_on_original_root() {
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeWorkRequest,
    };
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut birth = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    birth.create().unwrap();
    #[cfg(unix)]
    let target_path = fixture.0.join(birth.generation_name());
    let epoch = db.begin_read_transaction().unwrap().commit_epoch();
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    let mut lease = sealed_native_candidate_document_lease(
        &mut birth,
        &fixture,
        epoch,
        &governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: "amber ".repeat(384 * 1024),
            embedding: Some(vec![1.0, 0.0]),
            metadata: Default::default(),
        },
    );
    lease
        .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
        .unwrap();
    let candidate = lease
        .detach_candidate()
        .unwrap()
        .attach_birth(birth)
        .unwrap();
    #[cfg(unix)]
    {
        std::fs::rename(&target_path, target_path.with_extension("original")).unwrap();
        std::fs::create_dir(&target_path).unwrap();
        std::fs::write(target_path.join("foreign"), b"untouched").unwrap();
    }
    let before = governor.snapshot().admitted_memory_bytes;
    // The vector payload is tiny. Only late hydration must decode a document
    // whose encoded record exceeds this operation's small working grant.
    for budget in [3 * 1024 * 1024, 16 * 1024 * 1024] {
        let permit = governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(budget)
                    .with_result_bytes(1024 * 1024)
                    .with_io_wave_slots(1),
            )
            .unwrap();
        let result = candidate.probe_candidate_query(
            &permit,
            hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(30)),
            "",
            Some(&[1.0, 0.0]),
            hawdb_search::SearchMode::Vector,
            hawdb_search::SearchQueryOptions {
                limit: 1,
                offset: 0,
                rank_window: None,
                fusion_weights: Default::default(),
                metadata_filters: Default::default(),
                policy_epoch: None,
            },
        );
        if budget == 3 * 1024 * 1024 {
            assert!(
                matches!(
                    result,
                    Err(crate::SearchQueryError::Query(
                        HawDBError::ResourceBudgetExceeded(_)
                    ))
                ),
                "{result:?}"
            );
        } else {
            let output = result.unwrap();
            assert_eq!(output.result().total_hits, 1);
            assert_eq!(output.result().hits[0].id, "memory:m1");
            assert!(output.metrics().hydrated_bytes > 2 * 1024 * 1024);
            assert!(output.metrics().hydration_segment_bytes_read > 0);
        }
        drop(permit);
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
        assert_eq!(governor.snapshot().admitted_memory_bytes, before);
    }
    drop(candidate.abandon());
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    #[cfg(unix)]
    {
        assert_eq!(
            std::fs::read(target_path.join("foreign")).unwrap(),
            b"untouched"
        );
        assert_eq!(std::fs::read_dir(target_path).unwrap().count(), 1);
    }
}

#[test]
#[cfg(feature = "full-text-search")]
fn consumer_native_candidate_view_retains_results_after_handles_close() {
    use crate::SearchQueryError;
    use hawdb_qos::{
        IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig, RuntimeRetainedResultError,
        RuntimeWorkRequest,
    };
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let mut birth = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    birth.create().unwrap();
    let epoch = db.begin_read_transaction().unwrap().commit_epoch();
    let governor = RuntimeGovernor::detect(
        RuntimeGovernorConfig {
            memory_budget_bytes: Some(64 * 1024 * 1024),
            retained_result_handle_limit: std::num::NonZeroUsize::new(1).unwrap(),
            ..RuntimeGovernorConfig::shared_host()
        },
        IoConcurrencyBudget::new(2, 1),
    );
    let mut lease = sealed_native_candidate_document_lease(
        &mut birth,
        &fixture,
        epoch,
        &governor,
        hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "GraphStream".into(),
            content: "graph_stream Graph graph".into(),
            embedding: None,
            metadata: std::collections::BTreeMap::from([
                ("kind".into(), "Memory".into()),
                ("space_id".into(), "team".into()),
                ("external_id".into(), "m1 ".repeat(4096)),
                ("source_id".into(), "s1".into()),
            ]),
        },
    );
    lease
        .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
        .unwrap();
    let candidate = lease
        .detach_candidate()
        .unwrap()
        .attach_birth(birth)
        .unwrap();
    let baseline = governor.snapshot().admitted_memory_bytes;
    let options = || hawdb_search::SearchQueryOptions {
        limit: 1,
        offset: 0,
        rank_window: None,
        fusion_weights: Default::default(),
        metadata_filters: std::collections::BTreeMap::from([("space_id".into(), "team".into())]),
        policy_epoch: None,
    };
    let query = |permit: &hawdb_qos::RuntimePermit| {
        candidate.probe_candidate_query(
            permit,
            hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(10)),
            "graph",
            None,
            hawdb_search::SearchMode::Text,
            options(),
        )
    };
    let no_result = governor
        .try_admit(
            RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024).with_io_wave_slots(0),
        )
        .unwrap();
    assert!(matches!(
        query(&no_result),
        Err(SearchQueryError::Retention(
            RuntimeRetainedResultError::Capacity {
                retryable: false,
                ..
            }
        ))
    ));
    drop(no_result);
    assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, baseline);
    let small = governor
        .try_admit(
            RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                .with_result_bytes(8192)
                .with_io_wave_slots(1),
        )
        .unwrap();
    assert!(matches!(
        query(&small),
        Err(SearchQueryError::Query(HawDBError::ResourceBudgetExceeded(
            _
        )))
    ));
    drop(small);
    assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, baseline);
    let admit = || {
        governor
            .try_admit(
                RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024)
                    .with_result_bytes(1024 * 1024)
                    .with_io_wave_slots(1),
            )
            .unwrap()
    };
    for (limit, offset) in [(0, 0), (1, 10)] {
        let permit = admit();
        let mut page_options = options();
        page_options.limit = limit;
        page_options.offset = offset;
        let page = candidate
            .probe_candidate_query(
                &permit,
                hawdb_core::RuntimeTaskContext::with_timeout(std::time::Duration::from_secs(10)),
                "graph",
                None,
                hawdb_search::SearchMode::Text,
                page_options,
            )
            .unwrap();
        drop(permit);
        assert!(page.result().hits.is_empty());
        assert_eq!(page.result().total_hits, 1);
        assert_eq!(
            page.result().empty_reason_codes,
            [crate::SearchEmptyReasonCode::LimitExcludedAllHits]
        );
        if offset == 0 {
            assert!(page.result().truncated);
            assert_eq!(page.result().truncation_reason_codes.len(), 1);
            assert_eq!(
                page.result().truncation_reasons,
                ["limit 0 returned from 1 matching hits"]
            );
            assert_eq!(
                page.result().empty_reasons,
                page.result().truncation_reasons
            );
        } else {
            assert!(!page.result().truncated);
            assert_eq!(
                page.result().empty_reasons,
                ["offset 10 limit 1 returned no hits from 1 matching hits"]
            );
        }
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().active_background_io_slots, 0);
        drop(page);
        assert_eq!(governor.snapshot().admitted_memory_bytes, baseline);
    }
    let permit = admit();
    let retained = query(&permit).unwrap();
    drop(permit);
    assert_eq!(retained.result().hits[0].id, "memory:m1");
    assert_eq!(retained.result().hits[0].matched_terms, ["graph"]);
    assert!(retained.result().hits[0]
        .matched_spans
        .iter()
        .any(|span| span.text == "GraphStream"));
    assert_eq!(
        retained
            .result()
            .candidate_set
            .metadata_predicate_pushdown
            .field_summaries[0]
            .field,
        "space_id"
    );
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    let held = governor.retained_result_snapshot();
    assert_eq!(held.view_handles, 1);
    let second = admit();
    let error = query(&second).unwrap_err();
    assert!(error.is_retryable());
    assert!(matches!(error, SearchQueryError::Retention(_)));
    drop(second);
    assert_eq!(
        governor.retained_result_snapshot().retained_bytes,
        held.retained_bytes
    );
    assert_eq!(retained.result().hits[0].source_id.as_deref(), Some("s1"));
    drop(retained);
    assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    let permit = admit();
    let retained = query(&permit).unwrap();
    drop(permit);
    drop(candidate.abandon());
    drop(db);
    assert_eq!(
        retained.result().hits[0]
            .external_id
            .as_ref()
            .unwrap()
            .len(),
        3 * 4096
    );
    assert!(retained.retained_capacity_bytes() > 0);
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().active_background_io_slots, 0);
    assert_eq!(
        governor.snapshot().admitted_memory_bytes,
        governor.retained_result_snapshot().retained_bytes
    );
    drop(retained);
    assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
}

#[test]
fn consumer_native_candidate_view_refuses_native_operational_markers() {
    use hawdb_qos::{IoConcurrencyBudget, RuntimeGovernor, RuntimeGovernorConfig};
    for marker in [
        hawdb_search::FULL_REINDEX_MARKER,
        hawdb_search::METADATA_REPAIR_MARKER,
    ] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let mut birth = reserve_native_generation(&mut db, &fixture)
            .prepare_stationary_generation(&fixture.0)
            .unwrap();
        birth.create().unwrap();
        let epoch = db.begin_read_transaction().unwrap().commit_epoch();
        let path = fixture.0.join(birth.generation_name());
        let governor = RuntimeGovernor::detect(
            RuntimeGovernorConfig {
                memory_budget_bytes: Some(64 * 1024 * 1024),
                ..RuntimeGovernorConfig::shared_host()
            },
            IoConcurrencyBudget::new(2, 1),
        );
        let mut lease =
            sealed_native_candidate_lease(&mut birth, &fixture, epoch, &governor, "amber");
        #[cfg(unix)]
        let path = {
            let captured = path.with_extension("captured");
            std::fs::rename(&path, &captured).unwrap();
            std::fs::create_dir(&path).unwrap();
            captured
        };
        std::fs::write(path.join(marker), b"rebuild required").unwrap();
        let error = lease
            .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
            .unwrap_err();
        assert!(
            error.to_string().contains("contains operational marker"),
            "{error}"
        );
        std::fs::remove_file(path.join(marker)).unwrap();
        let report = lease
            .prepare_candidate_view(NonZeroU64::new(32 * 1024 * 1024).unwrap())
            .unwrap();
        assert_eq!(report.source_epoch, epoch);
        assert_eq!(report.document_count, 1);
    }
}

#[test]
fn consumer_native_candidate_metadata_refuses_a_changed_child_before_sealing() {
    use hawdb_search::consumer::generation_metadata::MetadataInput;

    const MANIFEST: &str = "search_projection.out_of_core.manifest.hawdb";
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let target_path = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("candidate-metadata-changed-stage");
    std::fs::create_dir(&stage_path).unwrap();
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let original = b"manifest one";
    let changed = b"manifest two";
    assert_eq!(original.len(), changed.len());
    std::fs::write(stage_path.join("manifest"), original).unwrap();
    let (binding, target_incarnation, generation, operation) =
        candidate_metadata_identity(&pending);
    let input = MetadataInput {
        binding: &binding,
        source_epoch: 7,
        target_incarnation,
        generation,
        operation,
        component_uuid: hawdb_core::Uuid::from_bytes([5; 16]),
        search_manifest: candidate_reference(original),
        artifacts: &[],
    };
    let limits = candidate_metadata_limits();
    let task = hawdb_core::RuntimeTaskContext::default();
    let metadata_len = input.encoded_len(limits, &task).unwrap() as u64;
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease
        .begin_candidate_output(original.len() as u64 + metadata_len)
        .unwrap();
    output
        .install_artifact(&stage, "manifest", MANIFEST, input.search_manifest)
        .unwrap();
    std::fs::write(target_path.join(MANIFEST), changed).unwrap();
    let error = output.seal_generation_metadata(&input, limits).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("differs from producer reference"),
        "{error}"
    );
    assert!(!target_path.join("consumer-generation.meta.hawdb").exists());
    assert_eq!(output.remaining_bytes(), metadata_len);
    assert!(output.seal_generation_metadata(&input, limits).is_err());
    drop(output);
    drop(lease);
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[test]
fn consumer_native_candidate_metadata_refuses_a_false_resident_absence_claim() {
    use hawdb_search::consumer::generation_metadata::{ArtifactReference, MetadataInput};

    const MANIFEST: &str = "search_projection.out_of_core.manifest.hawdb";
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let target_path = fixture.0.join(pending.generation_name());
    let context = pending.born_directory().unwrap().operation_context();
    let stage_path = fixture.0.join("candidate-absent-snapshot-stage");
    let mut writer = hawdb_search::SearchOutOfCoreGenerationWriter::create(
        &stage_path,
        hawdb_search::SearchOutOfCoreGenerationBuildOptions {
            source_graph_commit_epoch: Some(7),
            ..Default::default()
        },
    )
    .unwrap();
    writer
        .push(hawdb_search::SearchDocument {
            id: "memory:m1".into(),
            title: "candidate".into(),
            content: "absent resident snapshot".into(),
            embedding: None,
            metadata: Default::default(),
        })
        .unwrap();
    writer.finish().unwrap();
    let stage = hawdb_storage::file_io::SourceDirectory::capture(&stage_path, &context).unwrap();
    let manifest_bytes = std::fs::read(stage_path.join(MANIFEST)).unwrap();
    let mut names = std::fs::read_dir(&stage_path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(".1.hawdb"))
        .collect::<Vec<_>>();
    names.sort();
    let bytes = names
        .iter()
        .map(|name| std::fs::read(stage_path.join(name)).unwrap())
        .collect::<Vec<_>>();
    let artifacts = names
        .iter()
        .zip(&bytes)
        .map(|(name, bytes)| ArtifactReference {
            name,
            reference: candidate_reference(bytes),
        })
        .collect::<Vec<_>>();
    let (binding, target_incarnation, generation, operation) =
        candidate_metadata_identity(&pending);
    let input = MetadataInput {
        binding: &binding,
        source_epoch: 7,
        target_incarnation,
        generation,
        operation,
        component_uuid: hawdb_core::Uuid::from_bytes([5; 16]),
        search_manifest: candidate_reference(&manifest_bytes),
        artifacts: &artifacts,
    };
    let limits = candidate_metadata_limits();
    let task = hawdb_core::RuntimeTaskContext::default();
    let metadata_len = input.encoded_len(limits, &task).unwrap() as u64;
    let total = manifest_bytes.len() as u64
        + bytes.iter().map(|bytes| bytes.len() as u64).sum::<u64>()
        + metadata_len;
    let mut lease =
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(&mut pending, &task)
            .unwrap();
    let mut output = lease.begin_candidate_output(total).unwrap();
    output
        .install_artifact(&stage, MANIFEST, MANIFEST, input.search_manifest)
        .unwrap();
    for artifact in &artifacts {
        output
            .install_artifact(&stage, artifact.name, artifact.name, artifact.reference)
            .unwrap();
    }
    std::fs::write(
        target_path.join("search_projection.hawdb"),
        b"foreign snapshot",
    )
    .unwrap();
    let error = output.seal_generation_metadata(&input, limits).unwrap_err();
    assert!(error
        .to_string()
        .contains("present despite manifest absence proof"));
    assert!(!target_path.join("consumer-generation.meta.hawdb").exists());
    assert_eq!(output.remaining_bytes(), metadata_len);
    assert!(output.seal_generation_metadata(&input, limits).is_err());
    drop(output);
    drop(lease);
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[test]
fn consumer_native_birth_refuses_mutated_marker_before_exposing_target() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let marker = fixture
        .0
        .join(pending.generation_name())
        .join("consumer-birth.hawdb");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(marker)
        .unwrap();
    std::io::Write::write_all(&mut file, b"X").unwrap();
    file.sync_all().unwrap();
    let error = pending.born_directory().unwrap_err();
    assert!(
        error.to_string().contains("marker bytes changed"),
        "{error}"
    );
    assert!(
        hawdb_search::consumer::ConsumerStationaryTargetLease::acquire(
            &mut pending,
            &hawdb_core::RuntimeTaskContext::default(),
        )
        .is_err()
    );
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[cfg(unix)]
#[test]
fn consumer_native_birth_refuses_replaced_marker_with_identical_bytes() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let generation = fixture.0.join(pending.generation_name());
    let marker = generation.join("consumer-birth.hawdb");
    let bytes = std::fs::read(&marker).unwrap();
    std::fs::rename(&marker, generation.join("retained-original-marker")).unwrap();
    std::fs::write(&marker, bytes).unwrap();
    let error = pending.born_directory().unwrap_err();
    assert!(
        error.to_string().contains("marker identity changed"),
        "{error}"
    );
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Recorded"
    );
}

#[test]
fn consumer_native_birth_collision_never_adopts_matching_marker_or_forgets_debt() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let path = fixture.0.join(pending.generation_name());
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("consumer-birth.hawdb"), b"foreign marker").unwrap();
    assert!(pending.create().is_err());
    assert!(pending.born_directory().is_err());
    assert!(pending.create().is_err());
    drop(pending);
    assert_eq!(
        std::fs::read(path.join("consumer-birth.hawdb")).unwrap(),
        b"foreign marker"
    );
    drop(db);
    let _db = fixture.database();
    let evidence = allocation_evidence(&fixture).1;
    assert_eq!(evidence["high_water"], 3);
    assert_eq!(evidence["pending"][0]["native_birth"]["state"], "Pending");
}

#[cfg(unix)]
#[test]
fn consumer_native_birth_uses_captured_parent_after_target_label_replacement() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let parent = fixture.0.join("parent");
    let moved = fixture.0.join("retained-parent");
    std::fs::create_dir(&parent).unwrap();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation.prepare_stationary_generation(&parent).unwrap();
    let name = pending.generation_name().to_owned();
    std::fs::rename(&parent, &moved).unwrap();
    std::fs::create_dir_all(parent.join(&name)).unwrap();
    std::fs::write(parent.join(&name).join("foreign"), b"retain").unwrap();
    pending.create().unwrap();
    assert!(moved.join(&name).join("consumer-birth.hawdb").exists());
    assert!(!parent.join(&name).join("consumer-birth.hawdb").exists());
    assert_eq!(
        std::fs::read(parent.join(&name).join("foreign")).unwrap(),
        b"retain"
    );
    assert!(pending
        .born_directory()
        .unwrap()
        .matches_namespace(&moved.join(name))
        .unwrap());
}

#[test]
fn consumer_native_birth_fd_refusal_precedes_mkdir_and_retains_original_pending_record() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let name = pending.generation_name().to_owned();
    let before = allocation_evidence(&fixture).0;
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let capture = hawdb_storage::file_io::SourceDirectory::capture(&fixture.0, &context).unwrap();
    let mut blockers = Vec::new();
    loop {
        match capture.duplicate(&context) {
            Ok(blocker) => blockers.push(blocker),
            Err(error) => {
                assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
                break;
            }
        }
    }
    let error = pending.create().unwrap_err();
    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
    assert!(!fixture.0.join(name).exists());
    assert!(pending.born_directory().is_err());
    assert_eq!(allocation_evidence(&fixture).0, before);
    drop(blockers);
    // A fresh call must resolve the original attempt, not guess that an error
    // proves no effect and run another exclusive creation.
    assert!(pending.create().is_err());
    drop((capture, pending));
    assert_eq!(db.file_descriptor_metrics().unwrap().reserved, 0);
}

#[test]
fn consumer_allocation_precedes_callback_survives_failure_registry_loss_and_reopen() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 0);
    let result = db.create_search_projection_consumer(
        id("failed"),
        fixture.0.join("failed"),
        options(100),
        |_, _| {
            let evidence = allocation_evidence(&fixture).1;
            assert_eq!(evidence["high_water"], 3);
            assert_eq!(evidence["pending"][0]["consumer_id"], "failed");
            Err(HawDBError::Execution(
                "initializer failure after durable reserve".into(),
            ))
        },
    );
    assert!(result.is_err());
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 3);
    drop(db);
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("main"),
            fixture.0.join("projection"),
            options(100),
            initialize,
        )
        .unwrap();
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 6);
    drop(consumer);
    drop(db);
    std::fs::remove_file(fixture.registry_path("database")).unwrap();
    let mut db = fixture.database();
    let consumer = db
        .create_search_projection_consumer(
            id("replacement"),
            fixture.0.join("replacement"),
            options(100),
            initialize,
        )
        .unwrap();
    let evidence = allocation_evidence(&fixture).1;
    assert_eq!(evidence["high_water"], 9);
    assert_eq!(evidence["pending"].as_array().unwrap().len(), 3);
    assert_eq!(consumer.search_index().document_count(), 1);
}

#[test]
fn consumer_allocation_missing_or_corrupt_ledger_refuses_before_identity_and_callback() {
    for missing in [false, true] {
        let fixture = Fixture::new();
        let mut db = fixture.database();
        append(&mut db, 1);
        let ledger = fixture.0.join("database/consumer-allocations.hawdb");
        if missing {
            std::fs::remove_file(&ledger).unwrap();
        } else {
            std::fs::write(&ledger, b"damaged allocation evidence").unwrap();
        }
        let before = db
            .runtime
            .get()
            .unwrap()
            .store
            .search_projection_database_identity();
        let destination = fixture.0.join("never-created");
        assert!(db
            .create_search_projection_consumer(
                id("blocked"),
                &destination,
                options(100),
                |_, _| panic!("missing allocation evidence must reject before callback")
            )
            .is_err());
        assert!(!destination.exists());
        assert_eq!(
            db.runtime
                .get()
                .unwrap()
                .store
                .search_projection_database_identity(),
            before
        );
        if missing {
            assert!(!ledger.exists());
        } else {
            assert_eq!(
                std::fs::read(&ledger).unwrap(),
                b"damaged allocation evidence"
            );
        }
        append(&mut db, 2);
    }
}

#[test]
fn consumer_allocation_missing_evidence_revokes_source_readiness_and_activation() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let projection = fixture.0.join("projection");
    let consumer = db
        .create_search_projection_consumer(id("main"), &projection, options(100), initialize)
        .unwrap();
    assert!(db
        .search_projection_consumer_readiness(&consumer, None)
        .unwrap()
        .is_ready());
    std::fs::remove_file(fixture.0.join("database/consumer-allocations.hawdb")).unwrap();
    assert!(db
        .search_projection_consumer_readiness(&consumer, None)
        .is_err());
    assert!(db.search_projection_consumer_status(&id("main")).is_err());
    drop(consumer);
    assert!(db
        .open_search_projection_consumer(&id("main"), &projection)
        .is_err());
    append(&mut db, 1);
}

#[test]
fn consumer_allocation_exhaustion_refuses_without_effect_or_counter_reset() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    append(&mut db, 1);
    let (bytes, _) = allocation_evidence(&fixture);
    let body = std::str::from_utf8(&bytes[..bytes.len() - 4]).unwrap();
    assert_eq!(body.matches("\"high_water\":0").count(), 1);
    let mut exhausted = body
        .replace("\"high_water\":0", &format!("\"high_water\":{}", u64::MAX))
        .into_bytes();
    exhausted.extend_from_slice(&hawdb_integrity::crc32c(&exhausted).get().to_le_bytes());
    let ledger = fixture.0.join("database/consumer-allocations.hawdb");
    std::fs::write(&ledger, &exhausted).unwrap();
    let destination = fixture.0.join("never-created");
    let error = db
        .create_search_projection_consumer(id("exhausted"), &destination, options(100), |_, _| {
            panic!("exhaustion must precede callback")
        })
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("allocation identities exhausted"),
        "{error}"
    );
    assert_eq!(std::fs::read(&ledger).unwrap(), exhausted);
    assert!(!destination.exists());
    assert!(!std::fs::read_dir(&fixture.0).unwrap().any(|entry| entry
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".stage")));
    append(&mut db, 2);
}

#[test]
fn legacy_registered_prefix_1043_refuses_before_hydration_and_reopens_complete_retry() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    db.query("CREATE (:Memory {id: 'm1', content: 'base'})")
        .unwrap();
    let path = fixture.0.join("projection");
    let mut consumer = db
        .create_search_projection_consumer(id("coherent"), &path, options(100), initialize)
        .unwrap();
    let base_epoch = consumer.projection().receipt().source_epoch;
    db.query("MATCH (m:Memory {id: 'm1'}) SET m.content = 'at-B'")
        .unwrap();
    let mut transaction = db.begin_transaction().unwrap();
    transaction
        .query("MATCH (m:Memory {id: 'm1'}) SET m.content = 'at-E'")
        .unwrap();
    transaction
        .query("CREATE (:Memory {id: 'm2', content: 'new-at-E'})")
        .unwrap();
    transaction.commit().unwrap();
    let hydrated = std::cell::Cell::new(false);
    let error = db
        .catch_up_search_projection_consumer(&mut consumer, 1, 32, 1, |_, _| {
            hydrated.set(true);
            Ok(SearchProjectionRelationalDelta::default())
        })
        .unwrap_err();
    assert!(matches!(
        error,
        Error::Database(HawDBError::ProjectionSourceEpochMismatch { .. })
    ));
    assert!(!hydrated.get());
    assert_eq!(
        consumer
            .search_index()
            .document("memory:m1")
            .unwrap()
            .content,
        "base"
    );
    assert_eq!(consumer.projection().receipt().source_epoch, base_epoch);
    assert!(catch_up(&mut db, &mut consumer).unwrap().catch_up.complete);
    let final_epoch = db.commit_epoch().unwrap();
    drop(consumer);
    let reopened = db
        .open_search_projection_consumer(&id("coherent"), &path)
        .unwrap();
    assert_eq!(reopened.search_index().document_count(), 2);
    assert_eq!(
        reopened
            .search_index()
            .document("memory:m1")
            .unwrap()
            .content,
        "at-E"
    );
    assert_eq!(
        reopened
            .search_index()
            .document("memory:m2")
            .unwrap()
            .content,
        "new-at-E"
    );
    assert_eq!(reopened.projection().receipt().source_epoch, final_epoch);
}

#[test]
fn consumer_native_settlement_revokes_creation_and_reuses_only_pending_capacity() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let name = pending.generation_name().to_owned();
    let before = db.file_descriptor_metrics().unwrap().open;
    pending.settle_no_effect().unwrap();
    assert_eq!(db.file_descriptor_metrics().unwrap().open, before - 1);
    assert!(!fixture.0.join(&name).exists());
    assert!(pending.create().is_err());
    assert!(pending.born_directory().is_err());
    let evidence = allocation_evidence(&fixture);
    assert_eq!(evidence.1["high_water"], 3);
    assert!(evidence.1["pending"].as_array().unwrap().is_empty());
    pending.settle_no_effect().unwrap();
    assert_eq!(allocation_evidence(&fixture).0, evidence.0);
    drop((pending, db));
    let mut db = fixture.database();
    let next = reserve_native_generation(&mut db, &fixture);
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 6);
    let next = next.prepare_stationary_generation(&fixture.0).unwrap();
    assert_ne!(next.generation_name(), name);
}

#[test]
fn consumer_native_settlement_collision_preserves_a_complete_matching_foreign_marker() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let path = fixture.0.join(pending.generation_name());
    let evidence = allocation_evidence(&fixture).1;
    let binding = serde_json::json!([evidence["project_id"], evidence["pending"][0]]);
    let mut marker = b"HAWDB_CONSUMER_BIRTH_V1\n".to_vec();
    marker.extend(serde_json::to_vec(&binding).unwrap());
    marker.extend(hawdb_integrity::crc32c(&marker).get().to_le_bytes());
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("consumer-birth.hawdb"), &marker).unwrap();
    assert!(pending.create().is_err());
    pending.settle_no_effect().unwrap();
    assert_eq!(
        std::fs::read(path.join("consumer-birth.hawdb")).unwrap(),
        marker
    );
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 3);
    assert!(allocation_evidence(&fixture).1["pending"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn consumer_native_settlement_refuses_an_actual_birth_and_preserves_native_and_ledger_evidence() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let path = fixture.0.join(pending.generation_name());
    let marker = std::fs::read(path.join("consumer-birth.hawdb")).unwrap();
    let ledger = allocation_evidence(&fixture).0;
    assert!(matches!(
        pending.settle_no_effect(),
        Err(HawDBError::Execution(_))
    ));
    assert!(pending.born_directory().is_ok());
    assert_eq!(
        std::fs::read(path.join("consumer-birth.hawdb")).unwrap(),
        marker
    );
    assert_eq!(allocation_evidence(&fixture).0, ledger);
}

#[test]
fn consumer_native_settlement_fd_refusal_keeps_retry_custody_after_revoking_creation() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let name = pending.generation_name().to_owned();
    let before = allocation_evidence(&fixture).0;
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let capture = hawdb_storage::file_io::SourceDirectory::capture(&fixture.0, &context).unwrap();
    let mut blockers = Vec::new();
    loop {
        match capture.duplicate(&context) {
            Ok(blocker) => blockers.push(blocker),
            Err(error) => {
                assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
                break;
            }
        }
    }
    let error = pending.settle_no_effect().unwrap_err();
    assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
    assert_eq!(allocation_evidence(&fixture).0, before);
    assert!(pending.create().is_err());
    assert!(!fixture.0.join(name).exists());
    drop((blockers, capture));
    pending.settle_no_effect().unwrap();
    assert!(allocation_evidence(&fixture).1["pending"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 3);
    assert_eq!(db.file_descriptor_metrics().unwrap().reserved, 0);
}

#[cfg(unix)]
#[test]
fn consumer_native_settlement_refuses_uncertain_creation_even_when_no_path_exists() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let parent = fixture.0.join("unlinked-parent");
    std::fs::create_dir(&parent).unwrap();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation.prepare_stationary_generation(&parent).unwrap();
    std::fs::remove_dir(&parent).unwrap();
    assert!(pending.create().is_err());
    let before = allocation_evidence(&fixture).0;
    assert!(matches!(
        pending.settle_no_effect(),
        Err(HawDBError::Execution(_))
    ));
    assert_eq!(allocation_evidence(&fixture).0, before);
    assert_eq!(
        allocation_evidence(&fixture).1["pending"][0]["native_birth"]["state"],
        "Pending"
    );
    assert!(!parent.exists());
}

#[cfg(unix)]
#[test]
fn consumer_native_settlement_updates_the_original_source_after_label_replacement() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let allocation = reserve_native_generation(&mut db, &fixture);
    let mut pending = allocation
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let source = fixture.0.join("database");
    let retained = fixture.0.join("retained-database");
    std::fs::rename(&source, &retained).unwrap();
    std::fs::create_dir(&source).unwrap();
    let foreign = source.join("consumer-allocations.hawdb");
    std::fs::write(&foreign, b"foreign source evidence").unwrap();
    pending.settle_no_effect().unwrap();
    assert_eq!(std::fs::read(foreign).unwrap(), b"foreign source evidence");
    let bytes = std::fs::read(retained.join("consumer-allocations.hawdb")).unwrap();
    let start = bytes.iter().position(|byte| *byte == b'\n').unwrap() + 1;
    let ledger: serde_json::Value = serde_json::from_slice(&bytes[start..bytes.len() - 4]).unwrap();
    assert_eq!(ledger["high_water"], 3);
    assert!(ledger["pending"].as_array().unwrap().is_empty());
}

#[test]
fn consumer_native_retirement_closes_unused_birth_before_settlement_without_refund() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut pending = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    let name = pending.generation_name().to_owned();
    pending.create().unwrap();
    assert!(fixture.0.join(&name).join("consumer-birth.hawdb").exists());
    pending.retire_unpublished().unwrap();
    assert!(!fixture.0.join(&name).exists());
    let evidence = allocation_evidence(&fixture);
    assert_eq!(evidence.1["high_water"], 3);
    assert!(evidence.1["pending"].as_array().unwrap().is_empty());
    assert!(pending.create().is_err());
    assert!(pending.born_directory().is_err());
    pending.retire_unpublished().unwrap();
    assert_eq!(allocation_evidence(&fixture).0, evidence.0);
    assert!(pending.settle_no_effect().is_err());
    drop((pending, db));
    let mut db = fixture.database();
    let _next = reserve_native_generation(&mut db, &fixture);
    assert_eq!(allocation_evidence(&fixture).1["high_water"], 6);
}

#[test]
fn consumer_native_retirement_refuses_even_a_previously_borrowed_producer_directory() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut pending = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let directory = pending.born_directory().unwrap();
    let context = directory.operation_context();
    let retained = directory.duplicate(&context).unwrap();
    let before = allocation_evidence(&fixture).0;
    assert!(pending.retire_unpublished().is_err());
    let marker = retained
        .open_child(&context, "consumer-birth.hawdb")
        .unwrap();
    assert!(marker.metadata().unwrap().len() > 32);
    drop((marker, retained));
    // Closing a producer's visible handles cannot revoke access it might have
    // handed onward. Selector/reader accounting is needed for this later case.
    assert!(pending.retire_unpublished().is_err());
    assert_eq!(allocation_evidence(&fixture).0, before);
    assert!(pending.born_directory().is_ok());
}

#[test]
fn consumer_native_retirement_retries_admission_without_exposing_or_forgetting_birth() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let mut pending = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&fixture.0)
        .unwrap();
    pending.create().unwrap();
    let before = allocation_evidence(&fixture).0;
    let context = db
        .runtime
        .get()
        .unwrap()
        .store
        .file_descriptor_context()
        .unwrap();
    let source = hawdb_storage::file_io::SourceDirectory::capture(&fixture.0, &context).unwrap();
    let mut blockers = Vec::new();
    loop {
        match source.duplicate(&context) {
            Ok(fd) => blockers.push(fd),
            Err(error) => {
                assert!(hawdb_core::error::file_descriptor_error(&error).is_some());
                break;
            }
        }
    }
    assert!(pending.retire_unpublished().is_err());
    assert!(pending.born_directory().is_err());
    assert!(pending.create().is_err());
    assert!(pending.settle_no_effect().is_err());
    drop(blockers);
    assert_eq!(allocation_evidence(&fixture).0, before);
    pending.retire_unpublished().unwrap();
    assert!(allocation_evidence(&fixture).1["pending"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[cfg(unix)]
#[test]
fn consumer_native_retirement_uses_original_parent_after_label_replacement() {
    let fixture = Fixture::new();
    let mut db = fixture.database();
    let parent = fixture.0.join("birth-parent");
    std::fs::create_dir(&parent).unwrap();
    let original = fixture.0.join("original-parent");
    let mut pending = reserve_native_generation(&mut db, &fixture)
        .prepare_stationary_generation(&parent)
        .unwrap();
    pending.create().unwrap();
    let name = pending.generation_name().to_owned();
    std::fs::rename(&parent, &original).unwrap();
    std::fs::create_dir(&parent).unwrap();
    std::fs::create_dir(parent.join(&name)).unwrap();
    std::fs::write(parent.join(&name).join("foreign"), b"keep").unwrap();
    pending.retire_unpublished().unwrap();
    assert!(!original.join(&name).exists());
    assert_eq!(
        std::fs::read(parent.join(&name).join("foreign")).unwrap(),
        b"keep"
    );
    assert!(allocation_evidence(&fixture).1["pending"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[cfg(windows)]
#[test]
fn windows_consumer_creation_refuses_before_callback_or_publication() {
    let fixture = Fixture::new();
    assert_consumer_publication_refusal(&fixture, fixture.database());
}

#[test]
fn disabled_consumer_publication_refuses_before_callback_or_publication() {
    let fixture = Fixture::new();
    let db = Database::open_with_config(
        fixture.0.join("database"),
        crate::DatabaseConfig {
            runtime_capabilities: hawdb_core::RuntimeCapabilities::shared_host().with(
                hawdb_core::RuntimeCapability::SearchProjectionPublication,
                false,
            ),
            ..crate::DatabaseConfig::default()
        },
    )
    .unwrap();
    assert_consumer_publication_refusal(&fixture, db);
}

fn assert_consumer_publication_refusal(fixture: &Fixture, mut db: Database) {
    append(&mut db, 1);
    let epoch = db.commit_epoch().unwrap();
    let identity = db
        .runtime
        .get()
        .unwrap()
        .store
        .search_projection_database_identity();
    let registry = fixture.registry_path("database");
    assert!(!registry.exists());
    let destination = fixture.0.join("projection");
    let names = || {
        let mut names = std::fs::read_dir(&fixture.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        names.sort();
        names
    };
    for occupied in [false, true] {
        if occupied {
            std::fs::create_dir(&destination).unwrap();
            std::fs::write(destination.join("foreign"), b"keep").unwrap();
        }
        let before = names();
        for _ in 0..2 {
            assert!(matches!(
                db.create_search_projection_consumer(
                    id("publication-refusal"),
                    &destination,
                    options(100),
                    |_, _| panic!("unsupported publication must refuse before initialization"),
                ),
                Err(Error::Database(HawDBError::CapabilityUnavailable {
                    capability: hawdb_core::RuntimeCapability::SearchProjectionPublication
                }))
            ));
            assert_eq!(names(), before);
            assert_eq!(db.commit_epoch().unwrap(), epoch);
            let runtime = db.runtime.get().unwrap();
            assert_eq!(
                runtime.store.search_projection_database_identity(),
                identity
            );
            assert!(runtime.projection_consumers.records.is_empty());
            assert!(runtime.projection_consumers.verified.is_empty());
            assert!(!registry.exists());
            if occupied {
                assert_eq!(std::fs::read(destination.join("foreign")).unwrap(), b"keep");
            } else {
                assert!(!destination.exists());
            }
        }
    }
}

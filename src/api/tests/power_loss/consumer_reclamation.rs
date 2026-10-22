// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use crate::{SearchProjectionConsumerId, SearchProjectionConsumerOptions, SearchRebuildOptions};
use std::io::Write;
use std::num::NonZeroU64;

#[test]
fn consumer_reclamation_persists_retirement_authority_before_the_first_unlink() {
    let fixture = Fixture::new();
    let mut database = open(&fixture.root, DurabilityPolicy::SyncOnEveryWrite);
    database
        .query("CREATE (:Memory {id: 'm1', content: 'document one'})")
        .unwrap();
    let id = SearchProjectionConsumerId::new("retirement-barrier").unwrap();
    let target = fixture.root.join("projection");
    let consumer = database
        .create_search_projection_consumer(
            id.clone(),
            &target,
            SearchProjectionConsumerOptions::new(NonZeroU64::new(100).unwrap()),
            |snapshot, index| {
                snapshot
                    .rebuild_search_projection(index, SearchRebuildOptions::default())
                    .map(|_| ())
            },
        )
        .unwrap();
    let ledger_path = fixture.root.join("consumer-allocations.hawdb");
    let selected = std::fs::read(&ledger_path).unwrap();
    database.unregister_search_projection_consumer(&id).unwrap();
    drop(consumer);
    let retired = std::fs::read(&ledger_path).unwrap();
    assert_ne!(selected, retired);

    // Model a lost final ledger barrier: the retirement is visible, while the
    // durable image still selects the generation about to be reclaimed.
    let mut file = hawdb_storage::file_io::File::create(&ledger_path).unwrap();
    file.write_all(&selected).unwrap();
    file.sync_all().unwrap();
    drop(file);
    let mut file = hawdb_storage::file_io::File::create(&ledger_path).unwrap();
    file.write_all(&retired).unwrap();
    drop(file);
    let ledger = Path::new("consumer-allocations.hawdb");
    let before = fixture.model.capture().unwrap();
    assert!(!before.uncovered_writes(ledger).unwrap().is_empty());
    assert_eq!(
        before.crash(&CrashPlan::default()).unwrap().bytes(ledger),
        Some(selected.as_slice())
    );
    fixture
        .model
        .observe(ObservationPoint {
            event: IoEvent::Remove,
            relative_path: "projection".into(),
            boundary: ObservationBoundary::Before,
            skip_matches: 0,
            include_descendants: true,
            keep_last: false,
        })
        .unwrap();
    let report = database
        .retry_search_projection_consumer_cleanup(&id, &target, &Default::default())
        .unwrap();
    assert_eq!(report.failed_generations, 0, "{report:?}");
    assert_eq!(report.reclaimed_generations, 1, "{report:?}");
    let unlink = fixture.model.take_observation().unwrap().unwrap();
    assert!(unlink.uncovered_writes(ledger).unwrap().is_empty());
    for plan in publication_fault_plans(&unlink) {
        assert_eq!(
            unlink.crash(&plan).unwrap().bytes(ledger),
            Some(retired.as_slice()),
            "retirement authority must survive every uncovered-write schedule: {plan:?}"
        );
    }
    let completed = fixture.model.capture().unwrap();
    let durable = completed.crash(&CrashPlan::default()).unwrap();
    assert!(!durable
        .directory_paths()
        .any(|path| path == Path::new("projection")));
    assert_eq!(durable.bytes(ledger), Some(retired.as_slice()));
}

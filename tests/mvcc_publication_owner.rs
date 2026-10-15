// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use hawdb::{
    Database, DatabasePublicationOwner, DatabasePublicationState, PreparedReadLimits,
    ProjectionPublicationError, SearchDocument, SearchOutOfCoreGenerationBuildOptions,
    SearchOutOfCoreGenerationWriter, SearchOutOfCoreReader, Value,
};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

struct Directory(PathBuf);
impl Directory {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "hawdb-mvcc-owner-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct State(Database);
impl DatabasePublicationState for State {
    fn database(&self) -> &Database {
        &self.0
    }
}
fn database(path: &std::path::Path) -> Database {
    let mut db = Database::open(path).unwrap();
    db.query("CREATE (:Memory {id: 'a', value: 1})").unwrap();
    db.query("CREATE (:Other {id: 'b', value: 1})").unwrap();
    db.query("CREATE (:Binding {incarnation: 'current'})")
        .unwrap();
    db
}
fn limits() -> PreparedReadLimits {
    PreparedReadLimits {
        dependency_entries: NonZeroUsize::new(32).unwrap(),
        dependency_bytes: NonZeroUsize::new(4096).unwrap(),
        statement_rows: NonZeroUsize::new(8).unwrap(),
        statement_payload_bytes: NonZeroUsize::new(4096).unwrap(),
    }
}

#[test]
fn owner_composes_private_preparation_with_source_and_binding_validation() {
    for mutation in ["disjoint", "related", "binding_aba", "replace"] {
        let root = Directory::new();
        let owner = Arc::new(DatabasePublicationOwner::new(State(database(&root.0))));
        let search = root.0.join("search");
        let (captured_tx, captured_rx) = mpsc::sync_channel(1);
        let (resume_tx, resume_rx) = mpsc::sync_channel(1);
        let worker_owner = Arc::clone(&owner);
        let worker_search = search.clone();
        let preparation = std::thread::spawn(move || {
            let mut read = worker_owner
                .with_read(|state| state.0.begin_prepared_read(limits()))
                .unwrap();
            // The binding is a host-owned graph row in this fixture. Its
            // captured dependency, rather than an equal latest value, fences ABA.
            let binding = read
                .query_with_params(
                    "MATCH (b:Binding) WHERE id(b) = $id RETURN b.incarnation AS incarnation",
                    &BTreeMap::from([("id".into(), Value::Int(2))]),
                )
                .unwrap();
            assert_eq!(
                binding.output.rows[0]["incarnation"],
                Value::String("current".into())
            );
            captured_tx.send(read.source_epoch()).unwrap();
            resume_rx.recv_timeout(Duration::from_secs(30)).unwrap();
            // Querying and artifact construction both happen after capture has
            // released the actual public owner, against the original snapshot.
            let rows = read
                .query_with_params(
                    "MATCH (n:Memory) WHERE id(n) = $id RETURN n.value AS value",
                    &BTreeMap::from([("id".into(), Value::Int(0))]),
                )
                .unwrap();
            assert_eq!(rows.output.rows[0]["value"], Value::Int(1));
            let epoch = read.source_epoch();
            let dependencies = read.finish().unwrap();
            let mut writer = SearchOutOfCoreGenerationWriter::create(
                &worker_search,
                SearchOutOfCoreGenerationBuildOptions {
                    source_graph_commit_epoch: Some(epoch),
                    ..Default::default()
                },
            )
            .unwrap();
            writer
                .push(SearchDocument {
                    id: "a".into(),
                    title: "a".into(),
                    content: "1".into(),
                    embedding: None,
                    metadata: BTreeMap::new(),
                })
                .unwrap();
            (dependencies, writer.prepare_publication().unwrap())
        });
        let captured = captured_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let (committed_tx, committed_rx) = mpsc::sync_channel(1);
        let writer_owner = Arc::clone(&owner);
        let replacement = root.0.join("replacement");
        let foreground = std::thread::spawn(move || {
            let epoch = writer_owner
                .with_write(|state| {
                    match mutation {
                        "disjoint" => {
                            state.0.query("MATCH (n:Other) SET n.value = 2")?;
                        }
                        "related" => {
                            state.0.query("MATCH (n:Memory) SET n.value = 2")?;
                        }
                        "binding_aba" => {
                            state
                                .0
                                .query("MATCH (b:Binding) SET b.incarnation = 'away'")?;
                            state
                                .0
                                .query("MATCH (b:Binding) SET b.incarnation = 'current'")?;
                        }
                        "replace" => state.0 = database(&replacement),
                        _ => unreachable!(),
                    }
                    state.0.commit_epoch()
                })
                .unwrap();
            committed_tx.send(epoch).unwrap();
        });
        // Completion, not just thread creation or a try-lock, proves that a
        // foreground write finishes while preparation remains paused.
        let committed = committed_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        if mutation == "replace" {
            assert_eq!(committed, captured);
        } else {
            assert!(committed > captured);
        }
        assert!(!search
            .join("search_projection.out_of_core.manifest.hawdb")
            .exists());
        resume_tx.send(()).unwrap();
        foreground.join().unwrap();
        let (dependencies, candidate) = preparation.join().unwrap();
        let result = owner
            .with_validated_publication(
                |snapshot| {
                    let result = snapshot
                        .query("MATCH (b:Binding) RETURN b.incarnation AS incarnation LIMIT 2")?;
                    assert_eq!(result.rows.len(), 1);
                    assert_eq!(
                        result.rows[0]["incarnation"],
                        Value::String("current".into())
                    );
                    Ok(())
                },
                |state| Ok(state.0.publish_prepared_search(&dependencies, candidate)),
            )
            .unwrap();
        match mutation {
            "disjoint" => {
                assert_eq!(result.unwrap().source_graph_commit_epoch, Some(captured));
                let reader = SearchOutOfCoreReader::open(&search).unwrap();
                assert_eq!(reader.source_graph_commit_epoch(), Some(captured));
                assert_eq!(reader.document_count(), 1);
            }
            "replace" => assert!(matches!(
                result,
                Err(ProjectionPublicationError::SourceChanged)
            )),
            _ => assert!(matches!(
                result,
                Err(ProjectionPublicationError::Dependency(_))
            )),
        }
        if mutation != "disjoint" {
            assert!(!search
                .join("search_projection.out_of_core.manifest.hawdb")
                .exists());
            assert!(SearchOutOfCoreReader::open(&search).is_err());
            assert!(!std::fs::read_dir(&search).unwrap().any(|entry| entry
                .unwrap()
                .file_type()
                .unwrap()
                .is_dir()));
        }
    }
}

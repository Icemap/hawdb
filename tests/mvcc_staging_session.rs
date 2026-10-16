// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.
use hawdb::*;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "hawdb-staging-session-{}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn nz(n: usize) -> NonZeroUsize {
    NonZeroUsize::new(n).unwrap()
}
fn feed() -> ProjectionFeedLimits {
    ProjectionFeedLimits {
        work: nz(64),
        items: nz(8),
        bytes: nz(4096),
    }
}
fn governor() -> RuntimeGovernor {
    let resources = RuntimeResourceSnapshot::from_parts(
        RuntimeResourceBudget::from_limits(nz(2), None, None),
        RuntimeMemorySnapshot::from_limits(
            Some(1024 * 1024 * 1024),
            Some(1024 * 1024 * 1024),
            None,
            None,
            None,
        ),
    );
    RuntimeGovernor::new(
        RuntimeGovernorConfig {
            background_task_limit: Some(nz(1)),
            memory_budget_bytes: Some(128 * 1024 * 1024),
            ..RuntimeGovernorConfig::shared_host()
        },
        resources,
        IoConcurrencyBudget::new(2, 1),
    )
}
fn admission() -> SearchGenerationAdmission {
    SearchGenerationAdmission::acquire(
        &governor(),
        RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024).with_io_wave_slots(1),
    )
    .unwrap()
}
fn config(after_epoch: u64) -> ProjectionStagingSessionConfig {
    ProjectionStagingSessionConfig {
        after_epoch,
        reads: PreparedReadLimits {
            dependency_entries: nz(64),
            dependency_bytes: nz(8192),
            statement_rows: nz(4),
            statement_payload_bytes: nz(4096),
        },
        feed: ProjectionFeedLimits {
            work: nz(10000),
            items: nz(1000),
            bytes: nz(65536),
        },
        mapping: ProjectionMappingLimits {
            calls: nz(128),
            queries: nz(64),
            reserved_rows: nz(256),
            reserved_payload_bytes: nz(1024 * 1024),
            queries_per_call: nz(1),
            input_bytes: nz(4096),
            continuation_bytes: nz(128),
        },
        build: Default::default(),
        sorting: SearchUnorderedMutationLimits {
            max_buffer_records: nz(1),
            max_buffer_payload_bytes: nz(16384),
            ..Default::default()
        },
        task: RuntimeTaskContext::default()
            .with_memory_reservation(RuntimeMemoryReservation::new(16 * 1024 * 1024, 0)),
    }
}
fn doc(id: &str, body: &str) -> SearchDocument {
    SearchDocument {
        id: id.into(),
        title: id.into(),
        content: body.into(),
        embedding: None,
        metadata: BTreeMap::new(),
    }
}
fn setup(cold: bool) -> (Root, Database, SearchOutOfCoreReader, u64) {
    let root = Root::new();
    let graph = root.0.join("graph");
    let search = root.0.join("search");
    let mut db = Database::open(&graph).unwrap();
    db.query("CREATE (:Memory {id:'a', value:1})").unwrap();
    db.query_sql("CREATE TABLE records (id TEXT PRIMARY KEY, body TEXT)")
        .unwrap();
    db.query_sql("INSERT INTO records (id,body) VALUES ('a','old')")
        .unwrap();
    let c = db.commit_epoch().unwrap();
    let mut base = SearchOutOfCoreGenerationWriter::create(
        &search,
        SearchOutOfCoreGenerationBuildOptions {
            source_graph_commit_epoch: Some(c),
            ..Default::default()
        },
    )
    .unwrap();
    base.push(doc("memory:a", "old")).unwrap();
    base.push(doc("record:a", "old")).unwrap();
    base.finish().unwrap();
    db.query("MATCH (n:Memory) SET n.value=2").unwrap();
    db.query_sql("UPDATE records SET body='pinned' WHERE id='a'")
        .unwrap();
    if cold {
        db.checkpoint().unwrap();
        drop(db);
        db = Database::open_with_config(
            &graph,
            DatabaseConfig {
                storage_residency_mode: StorageResidencyMode::OutOfCore,
                ..Default::default()
            },
        )
        .unwrap();
    }
    let reader = SearchOutOfCoreReader::open(search).unwrap();
    (root, db, reader, c)
}
fn fanout(
    context: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    state: &[u8],
) -> Result<ProjectionMappingStatus> {
    match input {
        ProjectionMappingInput::GraphNode { node_id, .. } => {
            let page = state.first().copied().unwrap_or(0);
            let row = context.query_with_params(
                "MATCH (n:Memory) WHERE id(n)=$id RETURN n.value AS value",
                &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
            )?;
            assert_eq!(row.output.rows[0]["value"], Value::Int(2));
            context.upsert(doc(&format!("fan:{page}"), "2"))?;
            if page < 2 {
                context.set_continuation(&[page + 1])?;
                return Ok(ProjectionMappingStatus::More);
            }
        }
        ProjectionMappingInput::RelationalKey { table, key, .. } => {
            assert_eq!(table, "records");
            let RelationalValue::Text(id) = &key.0[0] else {
                panic!("text PK")
            };
            let row = context.query_sql_with_params(
                "SELECT body FROM records WHERE id=$1",
                &[Value::String(id.clone())],
            )?;
            assert_eq!(row.rows[0]["body"], Value::String("pinned".into()));
            context.upsert(doc("record:a", "pinned"))?;
        }
        ProjectionMappingInput::DeleteDocument { document_id, .. } => {
            context.delete(document_id)?
        }
        ProjectionMappingInput::End { source_epoch } => {
            assert_eq!(source_epoch, context.source_epoch())
        }
    }
    Ok(ProjectionMappingStatus::Done)
}
fn complete(session: &mut ProjectionStagingSession<'_>) {
    for _ in 0..1000 {
        if session.advance(feed(), Default::default()).unwrap()
            == ProjectionStagingSessionStep::Complete
        {
            return;
        }
    }
    panic!("session did not complete")
}
fn unchanged(root: &Root, reader: &SearchOutOfCoreReader) {
    let current = SearchOutOfCoreReader::open(root.0.join("search")).unwrap();
    assert_eq!(current.generation(), reader.generation());
    assert_eq!(
        current.source_graph_commit_epoch(),
        reader.source_graph_commit_epoch()
    );
    assert_eq!(
        current
            .hydrate_documents(&["memory:a".into(), "record:a".into()])
            .unwrap()
            .documents
            .iter()
            .map(|d| d.content.as_str())
            .collect::<Vec<_>>(),
        ["old", "old"]
    );
}
#[test]
fn staging_session_owns_pinned_mixed_input_and_fanout_across_sort_yields() {
    for cold in [false, true] {
        let (root, mut db, reader, c) = setup(cold);
        let e = db.commit_epoch().unwrap();
        let mut session = db
            .begin_projection_staging_session(&reader, config(c), fanout, admission())
            .unwrap();
        db.query("MATCH (n:Memory) SET n.value=3").unwrap();
        db.query_sql("UPDATE records SET body='later' WHERE id='a'")
            .unwrap();
        db.checkpoint().unwrap();
        assert_eq!(session.source_epoch(), e);
        let mut first_calls = 0;
        for _ in 0..1000 {
            let step = session.advance(feed(), Default::default()).unwrap();
            if step == ProjectionStagingSessionStep::Mapping && first_calls < 3 {
                first_calls += 1;
                assert_eq!(session.feed_usage().items, 1);
            }
            unchanged(&root, &reader);
            if step == ProjectionStagingSessionStep::Complete {
                break;
            }
        }
        assert_eq!(first_calls, 3);
        let usage = session.mapping_usage();
        assert_eq!(
            (
                usage.calls,
                usage.queries,
                usage.reserved_rows,
                usage.reserved_payload_bytes
            ),
            (5, 4, 16, 16384)
        );
        let staged = session.finish_staging().unwrap();
        assert_eq!(staged.source_epoch(), e);
        assert_eq!(
            staged.expected_active_generation(),
            Some(reader.generation())
        );
        assert_eq!(staged.sorting_report().input_operations, 4);
        assert_eq!(staged.sorting_report().output_operations, 4);
        drop(staged);
        unchanged(&root, &reader);
    }
}
fn ignore_query(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    let _ = ctx.query_sql_with_params("SELECT absent FROM no_table", &[]);
    Ok(ProjectionMappingStatus::Done)
}
fn ignore_second_query(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    ctx.query_sql_with_params(
        "SELECT body FROM records WHERE id=$1",
        &[Value::String("a".into())],
    )?;
    let _ = ctx.query_sql_with_params(
        "SELECT body FROM records WHERE id=$1",
        &[Value::String("a".into())],
    );
    Ok(ProjectionMappingStatus::Done)
}
fn ignore_output(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    ctx.upsert(doc("one", "one"))?;
    let _ = ctx.upsert(doc("two", "two"));
    Ok(ProjectionMappingStatus::Done)
}
fn ignore_continuation(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    let _ = ctx.set_continuation(&[0; 129]);
    Ok(ProjectionMappingStatus::More)
}
fn no_progress(
    _: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    Ok(ProjectionMappingStatus::More)
}
#[test]
fn staging_session_ignored_errors_and_no_progress_are_sticky() {
    for mapper in [
        ignore_query as ProjectionMappingFunction,
        ignore_second_query,
        ignore_output,
        ignore_continuation,
        no_progress,
    ] {
        let (root, db, reader, c) = setup(false);
        let mut session = db
            .begin_projection_staging_session(&reader, config(c), mapper, admission())
            .unwrap();
        assert_eq!(
            session.advance(feed(), Default::default()).unwrap(),
            ProjectionStagingSessionStep::Input
        );
        assert!(session.advance(feed(), Default::default()).is_err());
        assert!(matches!(
            session.advance(feed(), Default::default()),
            Err(ProjectionFeedError::Failed)
        ));
        assert!(matches!(
            session.finish_staging(),
            Err(ProjectionFeedError::Failed)
        ));
        unchanged(&root, &reader);
    }
}
#[test]
fn staging_session_cumulative_mapping_admission_never_resets() {
    for budget in ["calls", "queries", "rows", "bytes", "input"] {
        let (root, db, reader, c) = setup(false);
        let mut cfg = config(c);
        match budget {
            "calls" => cfg.mapping.calls = nz(1),
            "queries" => cfg.mapping.queries = nz(1),
            "rows" => cfg.mapping.reserved_rows = nz(1),
            "bytes" => cfg.mapping.reserved_payload_bytes = nz(1),
            "input" => cfg.mapping.input_bytes = nz(1),
            _ => unreachable!(),
        }
        let mut session = db
            .begin_projection_staging_session(&reader, cfg, fanout, admission())
            .unwrap();
        let mut failed = false;
        for _ in 0..100 {
            if session.advance(feed(), Default::default()).is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "{budget}");
        assert!(matches!(
            session.finish_staging(),
            Err(ProjectionFeedError::Failed)
        ));
        unchanged(&root, &reader);
    }
}
fn panic_mapper(
    _: &mut ProjectionMappingContext<'_, '_, '_>,
    _: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    panic!("injected mapper panic")
}
#[test]
fn staging_session_panic_cancel_and_premature_finish_preserve_base() {
    let (root, db, reader, c) = setup(false);
    let session = db
        .begin_projection_staging_session(&reader, config(c), fanout, admission())
        .unwrap();
    assert!(matches!(
        session.finish_staging(),
        Err(ProjectionFeedError::Incomplete)
    ));
    let mut session = db
        .begin_projection_staging_session(&reader, config(c), panic_mapper, admission())
        .unwrap();
    session.advance(feed(), Default::default()).unwrap();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(
        || session.advance(feed(), Default::default())
    ))
    .is_err());
    assert!(matches!(
        session.finish_staging(),
        Err(ProjectionFeedError::Failed)
    ));
    for after_completion in [false, true] {
        let cfg = config(c);
        let cancel = cfg.task.cancellation().clone();
        let mut session = db
            .begin_projection_staging_session(&reader, cfg, fanout, admission())
            .unwrap();
        if after_completion {
            complete(&mut session);
        } else {
            session.advance(feed(), Default::default()).unwrap();
        }
        cancel.cancel();
        assert!(session.finish_staging().is_err());
        unchanged(&root, &reader);
    }
}
#[test]
fn staging_session_requires_memory_and_owns_watermark() {
    let (_root, db, reader, c) = setup(false);
    let mut cfg = config(c);
    cfg.task =
        RuntimeTaskContext::default().with_memory_reservation(RuntimeMemoryReservation::new(0, 0));
    assert!(db
        .begin_projection_staging_session(&reader, cfg, fanout, admission())
        .is_err());
    let mut cfg = config(c);
    cfg.build.source_graph_commit_epoch = Some(c);
    assert!(db
        .begin_projection_staging_session(&reader, cfg, fanout, admission())
        .is_err());
    let cfg = config(db.commit_epoch().unwrap() + 1);
    assert!(matches!(
        db.begin_projection_staging_session(&reader, cfg, fanout, admission()),
        Err(ProjectionFeedError::FutureCursor { .. })
    ));
    let cfg = config(c + 1);
    assert!(db
        .begin_projection_staging_session(&reader, cfg, fanout, admission())
        .is_err());
}

fn final_e(
    ctx: &mut ProjectionMappingContext<'_, '_, '_>,
    input: ProjectionMappingInput<'_>,
    _: &[u8],
) -> Result<ProjectionMappingStatus> {
    match input {
        ProjectionMappingInput::GraphNode { node_id, .. } => {
            let row = ctx.query_with_params(
                "MATCH (n:Memory) WHERE id(n)=$id RETURN n.id AS id",
                &BTreeMap::from([("id".into(), Value::Int(node_id as i64))]),
            )?;
            assert!(row.output.rows.is_empty());
        }
        ProjectionMappingInput::RelationalKey { .. } => {
            let row = ctx.query_sql_with_params(
                "SELECT body FROM records WHERE id=$1",
                &[Value::String("a".into())],
            )?;
            assert_eq!(row.rows[0]["body"], Value::String("at-E".into()));
            ctx.upsert(doc("record:a", "at-E"))?;
        }
        ProjectionMappingInput::DeleteDocument { document_id, .. } => ctx.delete(document_id)?,
        ProjectionMappingInput::End { .. } => {}
    }
    Ok(ProjectionMappingStatus::Done)
}
#[test]
fn staging_session_reduces_whole_interval_instead_of_publishing_a_prefix() {
    for cold in [false, true] {
        let (root, mut db, reader, c) = setup(cold);
        db.query("MATCH (n:Memory) DETACH DELETE n").unwrap();
        db.query_sql("UPDATE records SET body='at-E' WHERE id='a'")
            .unwrap();
        let e = db.commit_epoch().unwrap();
        let mut session = db
            .begin_projection_staging_session(&reader, config(c), final_e, admission())
            .unwrap();
        db.query("CREATE (:Memory {id:'a',value:99})").unwrap();
        db.query_sql("UPDATE records SET body='later' WHERE id='a'")
            .unwrap();
        db.checkpoint().unwrap();
        complete(&mut session);
        assert_eq!(session.feed_usage().items, 5);
        let staged = session.finish_staging().unwrap();
        assert_eq!(staged.source_epoch(), e);
        assert_eq!(staged.sorting_report().input_operations, 3);
        assert_eq!(staged.sorting_report().output_operations, 2);
        unchanged(&root, &reader);
        drop(staged);
        unchanged(&root, &reader);
    }
}

#[test]
fn staging_session_empty_interval_still_requires_mapped_end() {
    let root = Root::new();
    let mut db = Database::open(root.0.join("graph")).unwrap();
    db.query("CREATE (:Memory {id:'a',value:1})").unwrap();
    let epoch = db.commit_epoch().unwrap();
    let mut base = SearchOutOfCoreGenerationWriter::create(
        root.0.join("search"),
        SearchOutOfCoreGenerationBuildOptions {
            source_graph_commit_epoch: Some(epoch),
            ..Default::default()
        },
    )
    .unwrap();
    base.push(doc("memory:a", "old")).unwrap();
    base.finish().unwrap();
    let reader = SearchOutOfCoreReader::open(root.0.join("search")).unwrap();
    let mut session = db
        .begin_projection_staging_session(&reader, config(epoch), fanout, admission())
        .unwrap();
    assert_eq!(
        session.advance(feed(), Default::default()).unwrap(),
        ProjectionStagingSessionStep::Input
    );
    assert_eq!(session.mapping_usage().calls, 0);
    assert!(matches!(
        session.finish_staging(),
        Err(ProjectionFeedError::Incomplete)
    ));
    let mut session = db
        .begin_projection_staging_session(&reader, config(epoch), fanout, admission())
        .unwrap();
    complete(&mut session);
    assert_eq!(session.feed_usage().items, 1);
    let staged = session.finish_staging().unwrap();
    assert_eq!(staged.mapping_usage().calls, 1);
    assert_eq!(staged.mapping_usage().queries, 0);
    assert_eq!(staged.sorting_report().output_operations, 0);
    drop(staged);
    assert_eq!(
        SearchOutOfCoreReader::open(root.0.join("search"))
            .unwrap()
            .generation(),
        reader.generation()
    );
}

#[test]
fn staging_session_retains_governor_permit_through_stage_and_failure_cleanup() {
    let (root, db, reader, c) = setup(false);
    let governor = governor();
    let request =
        RuntimeWorkRequest::background_maintenance(16 * 1024 * 1024).with_io_wave_slots(1);
    let admission = SearchGenerationAdmission::acquire(&governor, request).unwrap();
    let mut session = db
        .begin_projection_staging_session(&reader, config(c), fanout, admission)
        .unwrap();
    assert_eq!(governor.snapshot().active_background_tasks, 1);
    assert!(SearchGenerationAdmission::acquire(&governor, request).is_err());
    complete(&mut session);
    let staged = session.finish_staging().unwrap();
    assert_eq!(governor.snapshot().active_background_tasks, 1);
    assert!(governor.snapshot().admitted_memory_bytes >= request.memory_bytes);
    assert!(SearchGenerationAdmission::acquire(&governor, request).is_err());
    drop(staged);
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    for mapper in [ignore_output as ProjectionMappingFunction, panic_mapper] {
        let admission = SearchGenerationAdmission::acquire(&governor, request).unwrap();
        let mut session = db
            .begin_projection_staging_session(&reader, config(c), mapper, admission)
            .unwrap();
        session.advance(feed(), Default::default()).unwrap();
        let attempted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            session.advance(feed(), Default::default())
        }));
        assert!(attempted.is_err() || attempted.unwrap().is_err());
        assert_eq!(governor.snapshot().active_background_tasks, 1);
        assert!(matches!(
            session.finish_staging(),
            Err(ProjectionFeedError::Failed)
        ));
        assert_eq!(governor.snapshot().active_background_tasks, 0);
        assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    }
    let admission = SearchGenerationAdmission::acquire(&governor, request).unwrap();
    let mut cfg = config(c);
    cfg.build.source_graph_commit_epoch = Some(c);
    assert!(db
        .begin_projection_staging_session(&reader, cfg, fanout, admission)
        .is_err());
    assert_eq!(governor.snapshot().active_background_tasks, 0);
    assert_eq!(governor.snapshot().admitted_memory_bytes, 0);
    unchanged(&root, &reader);
}

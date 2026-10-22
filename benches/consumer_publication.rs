// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Same-binary controls for the outer embedded publication owner. Timing starts
//! before with_write and again inside its callback. Callback time is a measured
//! lower bound on exclusive ownership, not the inner selector's lock duration.

#[path = "consumer_publication/workload.rs"]
mod workload;

use hawdb::*;
use serde_json::{json, Value as Json};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Barrier;
use std::time::Instant;

fn ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().try_into().unwrap()
}

fn latency(values: &[u64]) -> Json {
    if values.is_empty() {
        return json!({"count": 0, "samples_ns": values});
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let at = |percent: usize| sorted[(sorted.len() * percent).div_ceil(100) - 1];
    json!({"count": values.len(), "p50_ns": at(50), "p95_ns": at(95),
        "p99_ns": at(99), "max_ns": sorted.last(), "samples_ns": values})
}

fn exclusive<T>(
    owner: &DatabasePublicationOwner<Database>,
    f: impl FnOnce(&mut Database) -> T,
) -> (T, u64, u64) {
    let requested = Instant::now();
    owner
        .with_write(|database| {
            let entered = Instant::now();
            let wait = ns(requested);
            let result = f(database);
            Ok((result, wait, ns(entered)))
        })
        .unwrap()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Disabled,
    Legacy,
    Covered,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Legacy => "legacy",
            Self::Covered => "covered",
        }
    }
}

fn update(database: &mut Database, writer: usize, value: usize) {
    database
        .query_sql_with_params(
            "UPDATE foreground SET value=$1 WHERE id=$2",
            &[
                Value::Int(value.try_into().unwrap()),
                Value::Int(writer.try_into().unwrap()),
            ],
        )
        .unwrap();
}

fn attempt(
    owner: &DatabasePublicationOwner<Database>,
    consumer: &mut SearchProjectionConsumer,
    mode: Mode,
) -> Json {
    let old = consumer.projection().receipt().source_epoch;
    let started = Instant::now();
    let mut capture_wait = 0;
    let mut capture_held = 0;
    let mut preparation = 0;
    let mut captured_epoch = None;
    let (outcome, wait, held) = if mode == Mode::Legacy {
        exclusive(owner, |db| {
            workload::legacy(db, consumer);
            "published"
        })
    } else {
        let (session, wait, held) = exclusive(owner, |db| workload::begin(db, consumer));
        capture_wait = wait;
        capture_held = held;
        captured_epoch = Some(session.source_epoch());
        let outside = Instant::now();
        let covered = workload::prepare(session);
        preparation = ns(outside);
        exclusive(owner, |db| {
            match db.publish_search_projection_consumer_covered_update(
                consumer,
                covered,
                ProjectionSourceDurability::RequireKnownDurable,
            ) {
                Ok(_) => "published",
                Err(ProjectionConsumerCatchUpError::Publication(
                    ProjectionPublicationError::Dependency(_),
                )) => "post_e_refusal",
                Err(error) => panic!("unexpected publication failure: {error}"),
            }
        })
    };
    let new = consumer.projection().receipt().source_epoch;
    let (cleanup, cleanup_wait, cleanup_held) = exclusive(owner, |db| {
        db.cleanup_search_projection_consumer(consumer, &Default::default())
            .unwrap()
    });
    assert_eq!(cleanup.failed_generations, 0, "{cleanup:?}");
    json!({
        "outcome": outcome, "watermark_before": old, "watermark_after": new,
        "advanced": new > old, "elapsed_ns": ns(started),
        "captured_source_epoch": captured_epoch,
        "capture_wait_plus_entry_ns": capture_wait, "capture_exclusive_callback_ns": capture_held,
        "preparation_outside_owner_ns": preparation,
        "publication_wait_plus_entry_ns": wait, "publication_exclusive_callback_ns": held,
        "cleanup_wait_plus_entry_ns": cleanup_wait, "cleanup_exclusive_callback_ns": cleanup_held,
        "reclaimed_generations": cleanup.reclaimed_generations,
    })
}

fn run(
    root: PathBuf,
    mode: Mode,
    documents: usize,
    payload: usize,
    writers: usize,
    writes: usize,
    quiet: usize,
) -> Json {
    // create_dir refuses existing paths; cleanup is confined to this fixture.
    std::fs::create_dir(&root).unwrap();
    let (database, mut consumer) = workload::setup(&root, documents, payload, writers);
    let initial = consumer.projection().receipt().source_epoch;
    let owner = DatabasePublicationOwner::new(database);
    let ready = Barrier::new(writers + 2);
    let active = AtomicUsize::new(writers);
    let completed = AtomicUsize::new(0);
    let (workers, attempts, elapsed) = std::thread::scope(|scope| {
        let handles = (0..writers)
            .map(|writer| {
                let owner = &owner;
                let ready = &ready;
                let active = &active;
                let completed = &completed;
                scope.spawn(move || {
                    let mut samples = Vec::new();
                    ready.wait();
                    for sequence in (writer..writes).step_by(writers) {
                        let started = Instant::now();
                        let (_, wait, held) =
                            exclusive(owner, |db| update(db, writer, sequence + 1));
                        samples.push((ns(started), wait, held));
                        completed.fetch_add(1, Ordering::Release);
                    }
                    active.fetch_sub(1, Ordering::Release);
                    samples
                })
            })
            .collect::<Vec<_>>();
        let publisher = scope.spawn(|| {
            ready.wait();
            let mut attempts = Vec::new();
            if mode != Mode::Disabled {
                while active.load(Ordering::Acquire) != 0 && attempts.len() < 128 {
                    let before = completed.load(Ordering::Acquire);
                    let mut row = attempt(&owner, &mut consumer, mode);
                    row["writes_before"] = json!(before);
                    row["writes_after"] = json!(completed.load(Ordering::Acquire));
                    row["writers_active_after"] = json!(active.load(Ordering::Acquire));
                    attempts.push(row);
                }
            }
            attempts
        });
        let started = Instant::now();
        ready.wait();
        let workers = handles
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        let elapsed = ns(started);
        (workers, publisher.join().unwrap(), elapsed)
    });
    assert_eq!(completed.load(Ordering::Acquire), writes);
    let source_after_writers = owner.with_read(Database::commit_epoch).unwrap();
    assert_eq!(source_after_writers, initial + writes as u64);
    let watermark_after_writers = consumer.projection().receipt().source_epoch;
    let active_advances = attempts
        .iter()
        .filter(|row| row["advanced"] == true && row["writers_active_after"].as_u64().unwrap() > 0)
        .count();
    let mut quiet_samples = Vec::new();
    if mode != Mode::Disabled {
        // Successful publication timing is measured separately: rejected work
        // cannot serve as evidence of a faster successful critical section.
        for sequence in 0..quiet {
            owner
                .with_write(|db| {
                    update(db, 0, writes + sequence + 1);
                    Ok(())
                })
                .unwrap();
            let row = attempt(&owner, &mut consumer, mode);
            assert_eq!(row["outcome"], "published");
            assert_eq!(row["advanced"], true);
            quiet_samples.push(row);
        }
        assert_eq!(
            consumer.projection().receipt().source_epoch,
            owner.with_read(Database::commit_epoch).unwrap()
        );
    }
    let index = consumer.search_index().unwrap();
    assert_eq!(index.document_count(), documents);
    for id in 0..documents {
        assert_eq!(
            index.document(&format!("m{id}")).unwrap().content,
            "x".repeat(payload)
        );
    }
    let transaction_samples = workers.iter().flatten().map(|s| s.0).collect::<Vec<_>>();
    let waits = workers.iter().flatten().map(|s| s.1).collect::<Vec<_>>();
    let holds = workers.iter().flatten().map(|s| s.2).collect::<Vec<_>>();
    let publication = quiet_samples
        .iter()
        .map(|row| row["publication_exclusive_callback_ns"].as_u64().unwrap())
        .collect::<Vec<_>>();
    let result = json!({
        "schema": "consumer-publication-qualification-v1", "mode": mode.name(),
        "documents": documents, "payload_bytes_per_document": payload, "writers": writers, "writes": writes,
        "durability": "SyncOnEveryWrite", "automatic_checkpoint_max_age_seconds": 3600,
        "governor_memory_bytes": 512_u64 << 20, "background_tasks": 1, "io_slots": [4,1],
        "data_shape": "fixed Memory projection plus disjoint relational foreground counters",
        "control": "same-binary catch-up disabled / legacy full-owner / staged covered, cleanup after each enabled attempt",
        "elapsed_writer_ns": elapsed, "writes_per_second": writes as f64 * 1e9 / elapsed as f64,
        "foreground_write": latency(&transaction_samples), "writer_wait_plus_entry": latency(&waits),
        "writer_exclusive_callback": latency(&holds), "quiescent_publication_exclusive_callback": latency(&publication),
        "source_epoch_before": initial, "source_epoch_after_writers": source_after_writers,
        "watermark_after_concurrent_phase": watermark_after_writers, "watermark_advances_with_active_writers": active_advances,
        "publication_attempts": attempts, "quiescent_publication_samples": quiet_samples,
        "publication_attempt_limit": 128,
        "publication_attempt_limit_reached_with_active_writers": attempts.len() == 128
            && attempts.last().unwrap()["writers_active_after"].as_u64().unwrap() > 0,
        "writer_samples": workers, "verified_documents": documents,
        "simultaneous_progress_qualified": false,
        "qualification_note": "Thread activity and interleaved serialized publications do not prove progress with a committed post-E write. The deterministic post-E regression remains a refusal; quiescent samples cannot satisfy this gate.",
        "lock_measurement": "wait includes entry overhead; exclusive callback is a lower bound on outer owner hold time, not inner selector mutex time",
    });
    drop(consumer);
    drop(owner);
    std::fs::remove_dir_all(root).unwrap();
    result
}

fn main() {
    let mut mode = Mode::Covered;
    let mut writers = 1;
    let mut documents = 32;
    let mut payload = 128;
    let mut writes = 1024;
    let mut quiet = 32;
    let mut root = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--bench" {
            continue;
        }
        let value = args.next().expect("argument requires a value");
        match arg.as_str() {
            "--mode" => {
                mode = match value.as_str() {
                    "disabled" => Mode::Disabled,
                    "legacy" => Mode::Legacy,
                    "covered" => Mode::Covered,
                    _ => panic!("mode must be disabled, legacy or covered"),
                }
            }
            "--writers" => writers = value.parse().unwrap(),
            "--documents" => documents = value.parse().unwrap(),
            "--payload" => payload = value.parse().unwrap(),
            "--writes" => writes = value.parse().unwrap(),
            "--quiet-samples" => quiet = value.parse().unwrap(),
            "--root" => root = Some(PathBuf::from(value)),
            _ => panic!("unknown argument: {arg}"),
        }
    }
    assert!([1, 4, 8].contains(&writers));
    assert!((1..=4096).contains(&documents) && (1..=2048).contains(&payload));
    assert!((writers..=100_000).contains(&writes) && (1..=1024).contains(&quiet));
    let root = root.expect("--root must name a new fixture directory on the measured filesystem");
    println!(
        "{}",
        run(root, mode, documents, payload, writers, writes, quiet)
    );
}

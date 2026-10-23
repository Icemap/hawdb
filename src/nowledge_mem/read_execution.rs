// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::{
    nowledge_mem_query_report, try_admit_runtime, NowledgeMemEmbeddedStoreHandle,
    NowledgeMemGraphMode, NowledgeMemQueryOutput, NowledgeMemQueryReportInput,
    NowledgeMemQueryReportOptions, NowledgeMemReadOptions, SearchProjectionExternalReadOperator,
};
use crate::api::PreparedRuntimeQuery;
use crate::{
    DatabaseConfig, DatabaseReadTransaction, HawDBError, QueryRequest, QueryStreamOptions, Result,
    Value,
};
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{RuntimeGovernor, RuntimeWorkPriority, RuntimeWorkRequest};
use std::collections::BTreeMap;
use std::sync::Arc;

struct AdmissionSource {
    identity: crate::DatabaseInstanceIdentity,
    config: DatabaseConfig,
    governor: RuntimeGovernor,
    external_seed_io_slots: usize,
}

impl AdmissionSource {
    fn execution_request(
        &self,
        prepared: &PreparedRuntimeQuery,
        result_budget: u64,
    ) -> RuntimeWorkRequest {
        let request = prepared
            .admission()
            .runtime_work_request_for_snapshot(result_budget, self.governor.snapshot())
            .with_retained_memory_bytes(prepared.planning_memory_bytes());
        if prepared.uses_external_seed() {
            // The canonical graph plan cannot infer a host projection's physical
            // I/O depth. Admit its published capability without pinning its data.
            let slots = request.io_slots.max(self.external_seed_io_slots);
            request.with_io_wave_slots(slots)
        } else {
            request
        }
    }
}

impl NowledgeMemEmbeddedStoreHandle {
    fn query_admission_source(&self) -> Result<AdmissionSource> {
        let publication = self.lock_published_read()?;
        let publication = publication.as_ref().ok_or_else(source_changed)?;
        publication.view.snapshot.ensure_usable()?;
        Ok(AdmissionSource {
            identity: publication.instance_identity.clone(),
            config: publication.view.snapshot.config().clone(),
            governor: publication.governor.clone(),
            external_seed_io_slots: publication.external_seed_io_slots,
        })
    }

    fn prepare_admitted_query(
        &self,
        source: &AdmissionSource,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        task_context: &RuntimeTaskContext,
    ) -> Result<PreparedRuntimeQuery> {
        task_context
            .checkpoint()
            .map_err(|reason| HawDBError::Execution(format!("runtime task {reason}")))?;
        let planning = try_admit_runtime(
            &source.governor,
            crate::api::runtime_planning_request(cypher.len(), RuntimeWorkPriority::Foreground),
        )?;
        let prepared = {
            let view = {
                let publication = self.lock_published_read()?;
                let publication = publication.as_ref().ok_or_else(source_changed)?;
                if publication.instance_identity != source.identity
                    || publication.view.snapshot.config() != &source.config
                {
                    return Err(source_changed());
                }
                Arc::clone(&publication.view)
            };
            view.prepare(QueryRequest::new(cypher).with_params(parameters))?
        };
        // Keep preparation memory charged, but release the source view and
        // planning CPU/task slots before execution admission or host callbacks.
        Ok(prepared.with_planning_memory(
            planning
                .into_retained_memory()
                .expect("planning owns a fixed memory reservation"),
        ))
    }

    pub(super) fn with_admitted_graph_read<T>(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: &NowledgeMemReadOptions,
        task_context: &RuntimeTaskContext,
        execute: impl FnOnce(
            &mut DatabaseReadTransaction,
            PreparedRuntimeQuery,
            QueryStreamOptions,
            &RuntimeTaskContext,
            NowledgeMemGraphMode,
        ) -> Result<T>,
    ) -> Result<T> {
        // Admission invokes host telemetry. Retain only scalar/configuration
        // metadata here, never a publication guard or a physical generation.
        let source = self.query_admission_source()?;
        let governor = &source.governor;
        let result = (|| {
            task_context
                .checkpoint()
                .map_err(|reason| HawDBError::Execution(format!("runtime task {reason}")))?;
            let limits = QueryStreamOptions {
                max_rows: options.max_rows,
                max_payload_bytes: Some(admitted_result_bytes(&source.config, governor, options)?),
            };
            let prepared =
                self.prepare_admitted_query(&source, cypher, parameters, task_context)?;
            if prepared.admission().is_mutation {
                return Err(HawDBError::Execution(
                    "admitted streaming query must be read-only".into(),
                ));
            }
            let permit = try_admit_runtime(
                governor,
                prepared
                    .admission()
                    .runtime_work_request_for_snapshot(
                        u64::try_from(limits.max_payload_bytes.unwrap()).unwrap_or(u64::MAX),
                        governor.snapshot(),
                    )
                    .with_retained_memory_bytes(prepared.planning_memory_bytes()),
            )?;
            let context = permit.bind_task_context(task_context.clone());
            let (view, mode) = {
                let publication = self.lock_published_read()?;
                let publication = publication.as_ref().ok_or_else(source_changed)?;
                if publication.instance_identity != source.identity {
                    return Err(source_changed());
                }
                (Arc::clone(&publication.view), publication.mode)
            };
            // Ordinary data commits remain compatible. Schema, branch and
            // configuration changes require new preparation/admission.
            if !view.accepts_prepared(&prepared) {
                return Err(source_changed());
            }
            let mut read = view.begin_read_transaction_with_context(&context)?;
            drop(view);
            self.finish_canonical_read(execute(&mut read, prepared, limits, &context, mode))
        })();
        if result.is_err()
            && let Err(reason) = task_context.checkpoint()
        {
            governor.record_cancellation(reason);
        }
        result
    }

    pub(super) fn query_with_admitted_report(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: NowledgeMemQueryReportOptions,
        task_context: &RuntimeTaskContext,
    ) -> Result<NowledgeMemQueryOutput> {
        let source = self.query_admission_source()?;
        let result = (|| {
            let prepared =
                self.prepare_admitted_query(&source, cypher, parameters, task_context)?;
            let is_mutation = prepared.admission().is_mutation;
            let result_budget =
                materialized_result_budget(&source.config, &source.governor, is_mutation)?;
            let uses_external_seed = prepared.uses_external_seed();
            let permit = try_admit_runtime(
                &source.governor,
                source.execution_request(&prepared, result_budget),
            )?;
            let context = permit.bind_task_context(task_context.clone());
            let snapshot = {
                let publication = self.lock_published_read()?;
                let publication = publication.as_ref().ok_or_else(source_changed)?;
                if publication.instance_identity != source.identity
                    || !publication.view.accepts_prepared(&prepared)
                    || (uses_external_seed
                        && publication.external_seed_io_slots != source.external_seed_io_slots)
                {
                    return Err(source_changed());
                }
                prepared.uses_read_snapshot().then(|| {
                    (
                        Arc::clone(&publication.view),
                        publication.mode,
                        uses_external_seed
                            .then(|| publication.resident_projection.clone())
                            .flatten(),
                        publication
                            .out_of_core_projection
                            .as_ref()
                            .map(|projection| projection.read_snapshot()),
                    )
                })
            };
            let started = super::Instant::now();
            let (mode, (output, trace)) = if let Some((view, mode, resident, projection)) = snapshot
            {
                let resident = resident
                    .map(|snapshot| snapshot.retain_with_permit(&permit))
                    .transpose()?;
                let mut external = SearchProjectionExternalReadOperator {
                    projection: resident
                        .as_ref()
                        .map(|lease| super::ResidentProjectionRef::Snapshot(lease.snapshot())),
                    out_of_core_projection: projection.as_ref(),
                    vector_seed_execution_count: 0,
                };
                let result = view.query_prepared_with_trace(
                    prepared,
                    QueryRequest::new(cypher)
                        .with_params(parameters)
                        .with_task_context(&context),
                    options.capture_physical_plan,
                    &mut external,
                );
                (mode, self.finish_canonical_read(result)?)
            } else {
                // Non-snapshot statements still use the canonical owner. Reuse the admitted template
                // and reject changes made while waiting for that owner.
                let mut store = self.write_store()?;
                let database = store.graph.database();
                if database.instance_identity()? != source.identity
                    || !database.accepts_prepared_query(&prepared)?
                    || (uses_external_seed
                        && store.external_seed_io_slots() != source.external_seed_io_slots)
                {
                    return Err(source_changed());
                }
                let super::NowledgeMemEmbeddedStore {
                    graph,
                    search_projection,
                    out_of_core_search_projection,
                    ..
                } = &mut *store;
                let mut external = SearchProjectionExternalReadOperator {
                    projection: search_projection
                        .as_ref()
                        .map(super::ResidentProjectionRef::Live),
                    out_of_core_projection: out_of_core_search_projection.as_ref(),
                    vector_seed_execution_count: 0,
                };
                let result = graph
                    .db
                    .query_prepared_with_params_trace_and_external_context(
                        prepared,
                        parameters,
                        options.capture_physical_plan,
                        &mut external,
                        &context,
                    )?;
                (graph.mode, result)
            };
            if !is_mutation {
                context
                    .checkpoint()
                    .map_err(|reason| HawDBError::Execution(format!("runtime task {reason}")))?;
            }
            let report = nowledge_mem_query_report(NowledgeMemQueryReportInput {
                mode,
                statement: &trace.statement,
                trace: trace.optimizer_trace.as_ref(),
                plan_cache_lookup: trace.plan_cache_lookup,
                execution_profile: trace.execution_profile.as_ref(),
                output: &output,
                options,
                elapsed_micros: started.elapsed().as_micros(),
            });
            Ok(NowledgeMemQueryOutput { output, report })
        })();
        if result.is_err()
            && let Err(reason) = task_context.checkpoint()
        {
            source.governor.record_cancellation(reason);
        }
        result
    }
}

pub(super) fn materialized_result_budget(
    config: &DatabaseConfig,
    governor: &RuntimeGovernor,
    is_mutation: bool,
) -> Result<u64> {
    let governor_budget = governor.snapshot().limits.result_budget_bytes;
    if is_mutation {
        return Ok(governor_budget);
    }
    let configured = config.max_read_result_payload_bytes.ok_or_else(|| {
        HawDBError::Execution(
            "admitted materialized query requires max_read_result_payload_bytes".into(),
        )
    })?;
    let configured = u64::try_from(configured).unwrap_or(u64::MAX);
    if configured > governor_budget {
        return Err(HawDBError::Execution(format!(
            "admitted materialized query payload budget {configured} exceeds runtime result budget {governor_budget}"
        )));
    }
    Ok(configured)
}

pub(super) fn admitted_result_bytes(
    config: &DatabaseConfig,
    governor: &RuntimeGovernor,
    options: &NowledgeMemReadOptions,
) -> Result<usize> {
    let governor_budget =
        usize::try_from(governor.snapshot().limits.result_budget_bytes).unwrap_or(usize::MAX);
    let configured = config
        .max_read_result_payload_bytes
        .unwrap_or(governor_budget);
    let requested = options
        .max_estimated_payload_bytes
        .unwrap_or(governor_budget);
    let admitted = governor_budget.min(configured).min(requested);
    if admitted == 0 {
        return Err(HawDBError::Execution(
            "admitted streaming query requires a non-zero result byte budget".into(),
        ));
    }
    Ok(admitted)
}

fn source_changed() -> HawDBError {
    HawDBError::Execution("read source changed during admission; retry the query".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nowledge_mem::app_read_snapshot_tests::app_read_handle;
    use hawdb_qos::{RuntimeTelemetryEvent, RuntimeTelemetryEventKind, RuntimeWorkKind};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Mutex, Weak};
    use std::time::Duration;

    const READ: &str = "MATCH (t:Thread) RETURN t.title AS title";
    const WRITE: &str = "MATCH (t:Thread) SET t.title = 'Updated thread'";

    fn options() -> NowledgeMemReadOptions {
        NowledgeMemReadOptions {
            max_rows: Some(1),
            max_estimated_payload_bytes: Some(4096),
        }
    }

    fn graph_only_handle() -> NowledgeMemEmbeddedStoreHandle {
        let handle = app_read_handle();
        handle.write_store().unwrap().search_projection = None;
        handle
    }

    #[derive(Debug)]
    struct ReportWriter {
        armed: AtomicBool,
        completed: AtomicBool,
        start: mpsc::Sender<()>,
        done: Mutex<mpsc::Receiver<bool>>,
    }

    impl crate::TelemetrySink for ReportWriter {
        fn record_query(&self, event: crate::QueryTelemetry<'_>) {
            if event.statement_kind == "match_return" && self.armed.swap(false, Ordering::SeqCst) {
                assert!(event.success);
                assert_eq!(event.row_count, 1);
                self.start.send(()).unwrap();
                self.completed.store(
                    self.done
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap_or(false),
                    Ordering::SeqCst,
                );
            }
        }
    }

    #[test]
    fn report_admission_includes_only_executed_external_projection_io() {
        let handle = app_read_handle();
        {
            let mut store = handle.write_store().unwrap();
            let projection = store.search_projection.as_mut().unwrap();
            let mut config = projection.index().range_read_config();
            config.io_depth = std::num::NonZeroUsize::new(3).unwrap();
            projection.set_range_read_config(config);
        }
        let source = handle.query_admission_source().unwrap();
        assert_eq!(source.external_seed_io_slots, 3);
        let parameters = BTreeMap::from([("text".into(), Value::String("App".into()))]);
        let seed = "CALL text_search($text, topK := 1) YIELD id, score MATCH (m:Memory) RETURN m.id AS id, score";
        for (query, expected) in [
            (seed.to_owned(), 3),
            (format!("EXPLAIN ANALYZE {seed}"), 3),
            (format!("EXPLAIN {seed}"), 0),
            (READ.to_owned(), 0),
        ] {
            let prepared = handle
                .prepare_admitted_query(
                    &source,
                    &query,
                    &parameters,
                    &RuntimeTaskContext::default(),
                )
                .unwrap();
            let request = source.execution_request(&prepared, 4096);
            assert_eq!(request.io_slots, expected, "{query}");
            if expected != 0 {
                assert_eq!(
                    request.io_reservation_scope,
                    hawdb_qos::RuntimeIoReservationScope::Wave
                );
            }
            drop(prepared);
            assert_eq!(source.governor.snapshot().admitted_memory_bytes, 0);
        }
    }

    #[test]
    fn report_observation_allows_writer_commit_and_a_fresh_read() {
        const TEXT: &str = "CALL text_search($text, topK := 2) YIELD id, score MATCH (m:Memory) RETURN m.title AS title";
        for (handle, query, expected_title) in [
            (graph_only_handle(), READ, "App thread"),
            (app_read_handle(), READ, "App thread"),
            (app_read_handle(), TEXT, "Nearest memory"),
        ] {
            let writer_handle = handle.clone();
            let (start, started) = mpsc::channel();
            let (finished, done) = mpsc::channel();
            let writer = std::thread::spawn(move || {
                if started.recv().is_ok() {
                    let result = (|| {
                        writer_handle.with_transaction(|tx| tx.query(WRITE))?;
                        let current = writer_handle.query_with_report(READ)?;
                        Ok::<_, HawDBError>(
                            current.output.rows[0]["title"]
                                == Value::String("Updated thread".into()),
                        )
                    })();
                    let _ = finished.send(result.unwrap_or(false));
                }
            });
            let sink = Arc::new(ReportWriter {
                armed: AtomicBool::new(true),
                completed: AtomicBool::new(false),
                start,
                done: Mutex::new(done),
            });
            handle.set_telemetry_sink(Some(sink.clone())).unwrap();
            let result = handle.query_with_params_with_report_options(
                query,
                &BTreeMap::from([("text".into(), Value::String("App".into()))]),
                NowledgeMemQueryReportOptions {
                    capture_physical_plan: true,
                    ..Default::default()
                },
            );
            handle.set_telemetry_sink(None).unwrap();
            let completed = sink.completed.load(Ordering::SeqCst);
            drop(sink);
            writer.join().unwrap();
            let result = result.unwrap();
            assert!(
                completed,
                "report observation blocked writer or fresh reader"
            );
            assert_eq!(
                result.output.rows[0]["title"],
                Value::String(expected_title.into())
            );
            assert!(result.report.physical_plan_captured);
            assert_eq!(
                handle
                    .runtime_governor_snapshot()
                    .unwrap()
                    .admitted_memory_bytes,
                0
            );
        }
    }

    #[test]
    fn report_reuses_one_preparation_and_preserves_plan_and_output_accounting() {
        let handle = graph_only_handle();
        let stats = || {
            handle
                .read_store()
                .unwrap()
                .graph
                .database()
                .plan_cache_stats()
                .unwrap()
        };
        let before = stats();
        let cypher = "MATCH (t:Thread {id: $id}) RETURN t.title AS title";
        let first = handle
            .query_with_params_with_report_options(
                cypher,
                &BTreeMap::from([("id".into(), Value::String("thread-1".into()))]),
                NowledgeMemQueryReportOptions {
                    capture_physical_plan: true,
                    ..Default::default()
                },
            )
            .unwrap();
        let after_first = stats();
        assert_eq!(after_first.misses, before.misses + 1);
        assert_eq!(after_first.hits, before.hits);
        assert_eq!(first.report.plan_cache_lookup.as_deref(), Some("miss"));
        assert!(first.report.physical_plan_captured);
        assert_eq!(
            first.output.rows[0]["title"],
            Value::String("App thread".into())
        );
        let second = handle
            .query_with_params_with_report(
                cypher,
                &BTreeMap::from([("id".into(), Value::String("missing".into()))]),
            )
            .unwrap();
        let after_second = stats();
        assert_eq!(after_second.misses, after_first.misses);
        assert_eq!(after_second.hits, after_first.hits + 1);
        assert_eq!(second.report.plan_cache_lookup.as_deref(), Some("hit"));
        assert!(!second.report.physical_plan_captured);
        assert!(second.output.rows.is_empty());
        assert_eq!(
            handle
                .runtime_governor_snapshot()
                .unwrap()
                .admitted_memory_bytes,
            0
        );
    }

    #[test]
    fn streaming_callback_allows_writer_commit_and_a_fresh_read() {
        let handle = app_read_handle();
        let writer_handle = handle.clone();
        let (start, started) = mpsc::channel();
        let (completed, done) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            if started.recv().is_ok() {
                let result = writer_handle.with_transaction(|tx| tx.query(WRITE));
                let _ = completed.send(result);
            }
        });
        let result =
            handle.read_query_with_params_streaming(READ, &BTreeMap::new(), &options(), |row| {
                start.send(()).unwrap();
                done.recv_timeout(Duration::from_secs(5)).map_err(|_| {
                    HawDBError::Execution("stream callback blocked writer".into())
                })??;
                assert_eq!(row["title"], Value::String("App thread".into()));
                let current = handle.read_query(READ, &options())?;
                assert_eq!(
                    current.output.rows[0]["title"],
                    Value::String("Updated thread".into())
                );
                Ok(())
            });
        drop(start);
        writer.join().unwrap();
        result.unwrap();
        assert_eq!(
            handle
                .runtime_governor_snapshot()
                .unwrap()
                .admitted_memory_bytes,
            0
        );
    }

    #[derive(Debug)]
    struct AdmissionWriter {
        armed: AtomicBool,
        prior_view: Mutex<Weak<crate::api::PublishedConcurrentRead>>,
        prior_released: AtomicBool,
        start: mpsc::Sender<()>,
        done: Mutex<mpsc::Receiver<bool>>,
    }

    impl crate::TelemetrySink for AdmissionWriter {
        fn record_query(&self, _: crate::QueryTelemetry<'_>) {}

        fn record_runtime(&self, event: RuntimeTelemetryEvent) {
            if event.kind == RuntimeTelemetryEventKind::Admitted
                && event.work_kind == Some(RuntimeWorkKind::Query)
                && self.armed.swap(false, Ordering::SeqCst)
            {
                self.start.send(()).unwrap();
                let completed = self
                    .done
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap_or(false);
                self.prior_released.store(
                    completed && self.prior_view.lock().unwrap().upgrade().is_none(),
                    Ordering::SeqCst,
                );
            }
        }
    }

    #[test]
    fn report_rejects_projection_io_changes_during_admission() {
        let handle = app_read_handle();
        let writer_handle = handle.clone();
        let (start, started) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let writer = std::thread::spawn(move || {
            if started.recv().is_ok() {
                let result = writer_handle.write_store().map(|mut store| {
                    let projection = store.search_projection.as_mut().unwrap();
                    let mut config = projection.index().range_read_config();
                    config.io_depth =
                        std::num::NonZeroUsize::new(config.io_depth.get() + 1).unwrap();
                    projection.set_range_read_config(config);
                });
                let _ = finished.send(result.is_ok());
            }
        });
        let sink = Arc::new(AdmissionWriter {
            armed: AtomicBool::new(false),
            prior_view: Mutex::new(Weak::new()),
            prior_released: AtomicBool::new(false),
            start,
            done: Mutex::new(done),
        });
        handle.set_telemetry_sink(Some(sink.clone())).unwrap();
        *sink.prior_view.lock().unwrap() =
            Arc::downgrade(&handle.lock_published_read().unwrap().as_ref().unwrap().view);
        sink.armed.store(true, Ordering::SeqCst);
        let result = handle.query_with_params_with_report(
            "CALL text_search($text, topK := 1) YIELD id, score MATCH (m:Memory) RETURN m.id AS id, score",
            &BTreeMap::from([("text".into(), Value::String("App".into()))]),
        );
        handle.set_telemetry_sink(None).unwrap();
        let prior_released = sink.prior_released.load(Ordering::SeqCst);
        drop(sink);
        writer.join().unwrap();
        assert!(
            prior_released,
            "projection reconfiguration was blocked by preparation"
        );
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("read source changed during admission"));
        assert_eq!(
            handle
                .runtime_governor_snapshot()
                .unwrap()
                .admitted_memory_bytes,
            0
        );
        assert_eq!(
            handle
                .runtime_governor_snapshot()
                .unwrap()
                .active_foreground_io_slots,
            0
        );
    }

    #[derive(Clone, Copy, Debug)]
    enum AdmissionChange {
        Data,
        Schema,
        Instance,
    }

    #[test]
    fn admission_releases_the_preparation_view_and_validates_the_current_source() {
        for report in [false, true] {
            for change in [
                AdmissionChange::Data,
                AdmissionChange::Schema,
                AdmissionChange::Instance,
            ] {
                let handle = if report {
                    graph_only_handle()
                } else {
                    app_read_handle()
                };
                let writer_handle = handle.clone();
                let (start, started) = mpsc::channel();
                let (finished, done) = mpsc::channel();
                let writer = std::thread::spawn(move || {
                    if started.recv().is_ok() {
                        let result = match change {
                            AdmissionChange::Data => {
                                writer_handle.with_transaction(|tx| tx.query(WRITE).map(|_| ()))
                            }
                            AdmissionChange::Schema => writer_handle.with_transaction(|tx| {
                                tx.query("CREATE (:NewSchema {id: 1})").map(|_| ())
                            }),
                            AdmissionChange::Instance => {
                                writer_handle.write_store().map(|mut store| {
                                    *store.graph_mut().database_mut() =
                                        crate::Database::new_with_config(
                                            store.graph().database().config().clone(),
                                        );
                                })
                            }
                        };
                        let _ = finished.send(result.is_ok());
                    }
                });
                let sink = Arc::new(AdmissionWriter {
                    armed: AtomicBool::new(false),
                    prior_view: Mutex::new(Weak::new()),
                    prior_released: AtomicBool::new(false),
                    start,
                    done: Mutex::new(done),
                });
                handle.set_telemetry_sink(Some(sink.clone())).unwrap();
                *sink.prior_view.lock().unwrap() =
                    Arc::downgrade(&handle.lock_published_read().unwrap().as_ref().unwrap().view);
                sink.armed.store(true, Ordering::SeqCst);
                let result = if report {
                    handle.query_with_report(READ).map(|result| result.output)
                } else {
                    handle
                        .read_query(READ, &options())
                        .map(|result| result.output)
                };
                handle.set_telemetry_sink(None).unwrap();
                let prior_released = sink.prior_released.load(Ordering::SeqCst);
                drop(sink);
                writer.join().unwrap();
                assert!(
                    prior_released,
                    "preparation retained its publication: {change:?}"
                );
                match change {
                    AdmissionChange::Data => assert_eq!(
                        result.unwrap().rows[0]["title"],
                        Value::String("Updated thread".into()),
                    ),
                    AdmissionChange::Schema | AdmissionChange::Instance => assert!(result
                        .unwrap_err()
                        .to_string()
                        .contains("source changed during admission"),),
                }
                assert_eq!(
                    handle
                        .runtime_governor_snapshot()
                        .unwrap()
                        .admitted_memory_bytes,
                    0
                );
            }
        }
    }
}

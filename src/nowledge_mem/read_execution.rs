// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::{
    try_admit_runtime, NowledgeMemEmbeddedStoreHandle, NowledgeMemGraphMode, NowledgeMemReadOptions,
};
use crate::api::PreparedRuntimeQuery;
use crate::{
    DatabaseConfig, DatabaseReadTransaction, HawDBError, QueryRequest, QueryStreamOptions, Result,
    Value,
};
use hawdb_core::RuntimeTaskContext;
use hawdb_qos::{RuntimeGovernor, RuntimeWorkPriority};
use std::collections::BTreeMap;
use std::sync::Arc;

impl NowledgeMemEmbeddedStoreHandle {
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
        let (identity, config, governor) = {
            let publication = self.lock_published_read()?;
            let publication = publication.as_ref().ok_or_else(source_changed)?;
            publication.view.snapshot.ensure_usable()?;
            (
                publication.instance_identity.clone(),
                publication.view.snapshot.config().clone(),
                publication.governor.clone(),
            )
        };
        let result = (|| {
            task_context
                .checkpoint()
                .map_err(|reason| HawDBError::Execution(format!("runtime task {reason}")))?;
            let limits = QueryStreamOptions {
                max_rows: options.max_rows,
                max_payload_bytes: Some(admitted_result_bytes(&config, &governor, options)?),
            };
            let planning = try_admit_runtime(
                &governor,
                crate::api::runtime_planning_request(cypher.len(), RuntimeWorkPriority::Foreground),
            )?;
            let prepared = {
                let view = {
                    let publication = self.lock_published_read()?;
                    let publication = publication.as_ref().ok_or_else(source_changed)?;
                    if publication.instance_identity != identity
                        || publication.view.snapshot.config() != &config
                    {
                        return Err(source_changed());
                    }
                    Arc::clone(&publication.view)
                };
                view.prepare(QueryRequest::new(cypher).with_params(parameters))?
            };
            // Prepared templates retain their planning reservation, but no
            // graph view survives into execution admission or its callbacks.
            let prepared = prepared.with_planning_memory(
                planning
                    .into_retained_memory()
                    .expect("planning owns a fixed memory reservation"),
            );
            if prepared.admission().is_mutation {
                return Err(HawDBError::Execution(
                    "admitted streaming query must be read-only".into(),
                ));
            }
            let permit = try_admit_runtime(
                &governor,
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
                if publication.instance_identity != identity {
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

    #[derive(Clone, Copy, Debug)]
    enum AdmissionChange {
        Data,
        Schema,
        Instance,
    }

    #[test]
    fn admission_releases_the_preparation_view_and_validates_the_current_source() {
        for change in [
            AdmissionChange::Data,
            AdmissionChange::Schema,
            AdmissionChange::Instance,
        ] {
            let handle = app_read_handle();
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
            let result = handle.read_query(READ, &options());
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
                    result.unwrap().output.rows[0]["title"],
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

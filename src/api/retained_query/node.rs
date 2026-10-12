// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

//! Experimental single-match indexed node delivery. Native views reuse source
//! allocations; recursive node maps are not represented as Arrow arrays.
//! Planning/index-key workspace and foreign adapters remain separate gates.

use super::*;
use hawdb_core::schema::{RetainedLabelAllocation, RetainedLabelName};
use hawdb_core::Catalog;
use hawdb_plan_cypher::{PhysicalPlan, Predicate};
use hawdb_storage::read_view::GraphReadAllocation;
use hawdb_storage::store::{MaterializedNodePointRead, MaterializedNodePointSource};

#[derive(Debug)]
struct PrepaidSource(usize);
impl RetainedLabelAllocation for PrepaidSource {
    fn bytes(&self) -> usize {
        self.0
    }
}
impl GraphReadAllocation for PrepaidSource {
    fn bytes(&self) -> usize {
        self.0
    }
    fn grow(&mut self, _: usize) -> crate::Result<()> {
        Err(HawDBError::Execution(
            "immutable source admission cannot grow".into(),
        ))
    }
}

struct NodeOwner {
    node: MaterializedNodePointSource<PrepaidSource>,
    labels: Vec<RetainedLabelName<PrepaidSource>>,
    column: String,
    status: AtomicU8,
    account: QueryMemoryAccount,
    ledger: QueryMemoryLedger,
    adapter_metadata_bytes: usize,
    // Complete source and owner capacity stays charged until the last view.
    _memory: QueryMemoryLease,
}

/// Borrowed logical node fields, including the ordinary synthetic fields.
#[derive(Debug, Clone, Copy)]
pub enum RetainedNodeField<'a> {
    Value(&'a Value),
    Identity(i64),
    Labels(RetainedNodeLabels<'a>),
}

#[derive(Debug, Clone, Copy)]
pub struct RetainedNodeLabels<'a> {
    labels: &'a [RetainedLabelName<PrepaidSource>],
}
impl<'a> RetainedNodeLabels<'a> {
    pub fn iter(self) -> impl ExactSizeIterator<Item = &'a str> {
        self.labels.iter().map(RetainedLabelName::name)
    }
}

/// One independently admitted native view. Borrowed fields cannot outlive it.
/// Closing its cursor does not revoke immutable payload already delivered.
pub struct RetainedNodeView {
    source: Arc<NodeOwner>,
    _view_memory: Option<QueryMemoryLease>,
    // Destroy the last payload owner before releasing the runtime result charge.
    runtime: RuntimeRetainedResult,
}
impl std::fmt::Debug for RetainedNodeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RetainedNodeView")
            .field("column", &self.source.column)
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}
impl RetainedNodeView {
    pub fn column_name(&self) -> &str {
        &self.source.column
    }

    pub fn status(&self) -> RetainedQueryStatus {
        match self.source.status.load(Ordering::Acquire) {
            0 => RetainedQueryStatus::Open,
            1 => RetainedQueryStatus::Completed,
            2 => RetainedQueryStatus::Failed,
            _ => RetainedQueryStatus::Closed,
        }
    }

    pub fn field(&self, name: &str) -> Result<Option<RetainedNodeField<'_>>> {
        let node = self.source.node.node()?;
        Ok(match name {
            "_id" => Some(RetainedNodeField::Identity(node.id.0 as i64)),
            "labels" => Some(RetainedNodeField::Labels(RetainedNodeLabels {
                labels: &self.source.labels,
            })),
            name => node.properties.get(name).map(RetainedNodeField::Value),
        })
    }

    /// Properties shadowed by the synthetic fields are deliberately omitted.
    pub fn properties(&self) -> Result<impl Iterator<Item = (&str, &Value)>> {
        Ok(self
            .source
            .node
            .node()?
            .properties
            .iter()
            .filter(|(name, _)| name.as_str() != "_id" && name.as_str() != "labels")
            .map(|(name, value)| (name.as_str(), value)))
    }

    pub fn try_retain(&self) -> Result<Self> {
        self.source.node.node()?;
        let metadata = std::mem::size_of::<Self>()
            .checked_add(self.source.adapter_metadata_bytes)
            .ok_or(RetainedQueryError::SizeOverflow)?;
        let runtime = self.runtime.try_retain(metadata as u64)?;
        let memory = self
            .source
            .account
            .reserve(runtime.handle_bytes() as usize)?;
        Ok(Self {
            source: Arc::clone(&self.source),
            _view_memory: Some(memory),
            runtime,
        })
    }
}

/// One-row active pull, provisional until successful EOF. Multi-match seeks
/// refuse explicitly rather than silently truncating the logical query.
#[derive(Debug)]
pub struct RetainedNodeCursor {
    view: Option<RetainedNodeView>,
    delivered: bool,
    status: RetainedQueryStatus,
    profile: RetainedQueryProfile,
    terminal_error: Option<RetainedQueryError>,
    payload_bytes: usize,
}
impl RetainedNodeCursor {
    pub fn profile(&self) -> RetainedQueryProfile {
        self.profile
    }
    pub fn status(&self) -> RetainedQueryStatus {
        self.status
    }
    pub fn access_path(&self) -> &'static str {
        "retained_indexed_node_view"
    }
    pub fn next_node(&mut self) -> Result<Option<RetainedNodeView>> {
        if self.status == RetainedQueryStatus::Closed {
            return Err(RetainedQueryError::Closed);
        }
        if self.status == RetainedQueryStatus::Failed {
            return Err(self
                .terminal_error
                .clone()
                .expect("failed cursor has a cause"));
        }
        self.profile.pulls += 1;
        if self.delivered || self.view.is_none() {
            self.status = RetainedQueryStatus::Completed;
            if let Some(view) = self.view.take() {
                view.source.status.store(1, Ordering::Release);
            }
            return Ok(None);
        }
        let result = self.view.as_ref().expect("source checked").try_retain();
        match result {
            Ok(view) => {
                self.delivered = true;
                self.profile.visited_rows = 1;
                self.profile.predicate_selected_rows = 1;
                self.profile.emitted_rows = 1;
                self.profile.peak_outstanding_batches = 1;
                self.profile.output_payload_bytes = self.payload_bytes;
                self.profile.query_peak_bytes = view.source.ledger.snapshot().peak_bytes;
                Ok(Some(view))
            }
            Err(error) if error.is_retryable() => {
                self.profile.backpressure_events += 1;
                Err(error)
            }
            Err(error) => {
                self.status = RetainedQueryStatus::Failed;
                self.terminal_error = Some(error.clone());
                if let Some(view) = self.view.take() {
                    view.source.status.store(2, Ordering::Release);
                }
                Err(error)
            }
        }
    }
    pub fn close(&mut self) {
        if self.status == RetainedQueryStatus::Open {
            if let Some(view) = self.view.take() {
                view.source.status.store(3, Ordering::Release);
            }
            self.status = RetainedQueryStatus::Closed;
        }
    }
}
impl Drop for RetainedNodeCursor {
    fn drop(&mut self) {
        self.close();
    }
}

struct PointPlan<'a> {
    label: &'a str,
    property: &'a str,
    value: &'a Value,
    column: &'a str,
    empty: bool,
}
fn point_plan(plan: &PhysicalPlan) -> Option<PointPlan<'_>> {
    let (plan, empty) = match plan {
        PhysicalPlan::LimitExec {
            offset: 0,
            limit,
            input,
        } => (input.as_ref(), *limit == Some(0)),
        plan => (plan, false),
    };
    let PhysicalPlan::ProjectExec { items, input } = plan else {
        return None;
    };
    let [item] = items.as_slice() else {
        return None;
    };
    let ProjectionExpression::Variable { variable: returned } = &item.expression else {
        return None;
    };
    let (leaf, filter) = match input.as_ref() {
        PhysicalPlan::FilterExec { predicate, input } => (input.as_ref(), Some(predicate)),
        plan => (plan, None),
    };
    let PhysicalPlan::IndexNodeSeek {
        variable,
        label,
        property,
        value,
    } = leaf
    else {
        return None;
    };
    if variable != returned || matches!(value, Value::Null) {
        return None;
    }
    if let Some(predicate) = filter
        && !matches!(predicate, Predicate::PropertyEq { variable: v, property: p, value: x }
            if v == variable && p == property && x == value)
    {
        return None;
    }
    Some(PointPlan {
        label,
        property,
        value,
        column: &item.name,
        empty,
    })
}

impl Database {
    /// Experimental native full-node view for a single-match indexed Cypher
    /// projection. Other plans/layouts refuse; no Arrow or copied fallback.
    /// Planning and foreign workspace are not yet whole-operation qualified.
    pub fn query_with_params_retained_node(
        &self,
        cypher: &str,
        parameters: &BTreeMap<String, Value>,
        options: RetainedQueryOptions,
    ) -> Result<RetainedNodeCursor> {
        let state = self.runtime.get()?;
        state.store.ensure_usable()?;
        let prepared = query_runtime::parse_runtime_execution(cypher)?;
        super::super::query_work_request_for_statement(
            &self.system_variables,
            &prepared.statement,
        )?;
        let optimized = self.optimized_query_plan_for_request(
            cypher,
            &prepared.statement,
            parameters,
            None,
            super::super::PlanTraceMode::Template,
            None,
        )?;
        let plan =
            point_plan(&optimized.physical_plan).ok_or(RetainedQueryError::UnsupportedPlan)?;
        if state
            .store
            .try_scan_materialized_nodes_after(None, None)?
            .is_none()
        {
            return Err(RetainedQueryError::CopyRequired);
        }
        let mut profile = RetainedQueryProfile {
            source_snapshot_rows: state.store.node_count_for_label(None),
            ..Default::default()
        };
        let empty = || RetainedNodeCursor {
            view: None,
            delivered: false,
            status: RetainedQueryStatus::Open,
            profile,
            terminal_error: None,
            payload_bytes: 0,
        };
        if plan.empty {
            return Ok(empty());
        }
        let label = state
            .catalog
            .label_id(plan.label)
            .ok_or(RetainedQueryError::UnsupportedPlan)?;
        let mut found = state
            .store
            .seek_nodes_by_property(label, plan.property, plan.value);
        let Some(row) = found.next() else {
            return Ok(empty());
        };
        if found.next().is_some() {
            return Err(RetainedQueryError::UnsupportedPlan);
        }
        let view = admit_node(
            &state.store,
            &state.catalog,
            row.id,
            plan.column,
            NodeAdmission {
                config: &self.config,
                runtime: &self.retained_runtime,
                options,
            },
            &mut profile,
        )?;
        let payload_bytes = profile.output_payload_bytes;
        profile.output_payload_bytes = 0;
        Ok(RetainedNodeCursor {
            view: Some(view),
            delivered: false,
            status: RetainedQueryStatus::Open,
            profile,
            terminal_error: None,
            payload_bytes,
        })
    }
}

struct NodeAdmission<'a> {
    config: &'a DatabaseConfig,
    runtime: &'a RetainedRuntime,
    options: RetainedQueryOptions,
}

fn admit_node(
    store: &GraphStore,
    catalog: &Catalog,
    id: NodeId,
    column: &str,
    admission: NodeAdmission<'_>,
    profile: &mut RetainedQueryProfile,
) -> Result<RetainedNodeView> {
    let NodeAdmission {
        config,
        runtime,
        options,
    } = admission;
    let row = store.node(id).ok_or(RetainedQueryError::UnsupportedPlan)?;
    let symbol_bytes = row.labels.iter().try_fold(0usize, |total, id| {
        total
            .checked_add(
                catalog
                    .retained_label_name_capacity_bytes(*id)?
                    .unwrap_or(0),
            )
            .ok_or(RetainedQueryError::SizeOverflow)
    })?;
    let metadata = std::mem::size_of::<NodeOwner>()
        .checked_add(2 * std::mem::size_of::<usize>())
        .and_then(|bytes| {
            bytes.checked_add(
                row.labels
                    .len()
                    .checked_mul(std::mem::size_of::<RetainedLabelName<PrepaidSource>>())?,
            )
        })
        .and_then(|bytes| bytes.checked_add(column.len()))
        .and_then(|bytes| bytes.checked_add(options.adapter_metadata_bytes))
        .and_then(|bytes| bytes.checked_add(std::mem::size_of::<RetainedNodeCursor>() + 64))
        .ok_or(RetainedQueryError::SizeOverflow)?;
    let governor = runtime.governor(options.minimum_shared_handles.get())?;
    let allowance = governor.snapshot().limits.result_budget_bytes;
    let permit = governor
        .try_admit(RuntimeWorkRequest::foreground_query(0, allowance).with_blocking(false))?;
    let ledger = QueryMemoryLedger::new(config.execution_memory.query_memory_bytes);
    let account = ledger.account(
        QueryMemoryClass::ResultMaterialization,
        "retained_node",
        config.execution_memory.query_memory_bytes,
    );
    let mut charges = None;
    let read = store.materialized_node_point_read_with_admission(id, |row_bytes| -> Result<_> {
        let capacity = row_bytes
            .checked_add(symbol_bytes)
            .and_then(|bytes| bytes.checked_add(metadata))
            .ok_or(RetainedQueryError::SizeOverflow)?;
        if capacity
            > options
                .batch_bytes
                .get()
                .min(config.execution_memory.batch_payload_bytes.get())
        {
            return Err(RetainedQueryError::WorkingUnitTooLarge);
        }
        let mut payload = column
            .len()
            .checked_add("_id".len() + 8 + "labels".len())
            .ok_or(RetainedQueryError::SizeOverflow)?;
        for (key, value) in &row.properties {
            if key != "_id" && key != "labels" {
                payload = payload
                    .checked_add(key.len())
                    .and_then(|bytes| {
                        bytes.checked_add(hawdb_executor::query_value_payload_bytes(value))
                    })
                    .ok_or(RetainedQueryError::SizeOverflow)?;
            }
        }
        for label in &row.labels {
            payload = payload
                .checked_add(catalog.label_name(*label).map_or(0, str::len))
                .ok_or(RetainedQueryError::SizeOverflow)?;
        }
        if restrictive(config.max_read_result_rows, options.max_result_rows)
            .is_some_and(|limit| limit < 1)
            || restrictive(
                config.max_read_result_payload_bytes,
                options.max_result_payload_bytes,
            )
            .is_some_and(|limit| payload > limit)
        {
            return Err(RetainedQueryError::ResultBudget {
                rows: 1,
                payload_bytes: payload,
            });
        }
        let result = permit.reserve_retained_result(
            capacity as u64,
            std::mem::size_of::<RetainedNodeView>() as u64,
        )?;
        let memory = account.reserve(
            capacity
                .checked_add(RuntimeRetainedResult::owner_overhead_bytes() as usize)
                .and_then(|bytes| bytes.checked_add(result.handle_bytes() as usize))
                .ok_or(RetainedQueryError::SizeOverflow)?,
        )?;
        charges = Some((result, memory));
        profile.source_pinned_capacity_bytes = row_bytes + symbol_bytes;
        profile.output_payload_bytes = payload;
        Ok(PrepaidSource(row_bytes))
    })?;
    let MaterializedNodePointRead::Node(node) = read else {
        return Err(RetainedQueryError::CopyRequired);
    };
    let mut labels = Vec::with_capacity(row.labels.len());
    for id in &row.labels {
        if let Some(label) = catalog
            .retain_label_name_with_admission(*id, |bytes| -> Result<_> {
                Ok(PrepaidSource(bytes))
            })?
        {
            labels.push(label);
        }
    }
    let (result, memory) = charges.expect("point source was admitted");
    profile.source_pinned_rows = 1;
    profile.source_preflight_rows = 1;
    profile.query_peak_bytes = ledger.snapshot().peak_bytes;
    Ok(RetainedNodeView {
        source: Arc::new(NodeOwner {
            node,
            labels,
            column: column.to_owned(),
            status: AtomicU8::new(0),
            account,
            ledger,
            adapter_metadata_bytes: options.adapter_metadata_bytes,
            _memory: memory,
        }),
        _view_memory: None,
        runtime: result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUERY: &str = "MATCH (n:Item {score: $key}) RETURN n AS node";

    fn fixture(handles: usize) -> (Database, RuntimeGovernor) {
        let mut db = Database::new();
        db.query("CREATE NODE TABLE Item").unwrap();
        db.query("CREATE PROPERTY ON NODE TABLE Item(score) TYPE INT")
            .unwrap();
        db.query("CREATE INDEX ON :Item(score)").unwrap();
        for key in 0..128 {
            db.query_with_params(
                "CREATE (:Item {score: $key, text: $text, _id: 'shadow', labels: 'shadow'})",
                &BTreeMap::from([
                    ("key".into(), Value::Int(key)),
                    ("text".into(), Value::String("payload".into())),
                ]),
            )
            .unwrap();
        }
        let governor = RuntimeGovernor::new(
            RuntimeGovernorConfig {
                retained_result_handle_limit: NonZeroUsize::new(handles).unwrap(),
                ..RuntimeGovernorConfig::shared_host()
            },
            RuntimeResourceSnapshot::detect(),
            IoConcurrencyBudget::new(2, 1),
        );
        db.set_runtime_governor(governor.clone());
        (db, governor)
    }

    fn params() -> BTreeMap<String, Value> {
        BTreeMap::from([("key".into(), Value::Int(37))])
    }

    #[test]
    fn indexed_full_node_reuses_original_payload_and_preserves_shadowing() {
        let (mut db, governor) = fixture(4);
        let state = db.runtime.get().unwrap();
        let label = state.catalog.label_id("Item").unwrap();
        let row = state
            .store
            .seek_nodes_by_property(label, "score", &Value::Int(37))
            .next()
            .unwrap();
        let id = row.id.0 as i64;
        let value_address = &row.properties["text"] as *const Value as usize;
        let label_address = state.catalog.label_name(label).unwrap().as_ptr() as usize;
        let prepared = query_runtime::parse_runtime_execution(QUERY).unwrap();
        let optimized = db
            .optimized_query_plan_for_request(
                QUERY,
                &prepared.statement,
                &params(),
                None,
                super::super::super::PlanTraceMode::Template,
                None,
            )
            .unwrap();
        assert!(
            point_plan(&optimized.physical_plan).is_some(),
            "{:?}",
            optimized.physical_plan
        );
        let ordinary = db.query_with_params(QUERY, &params()).unwrap();
        let Some(Value::Map(expected)) = ordinary.rows[0].get("node") else {
            panic!("ordinary node map");
        };
        let mut cursor = db
            .query_with_params_retained_node(QUERY, &params(), RetainedQueryOptions::default())
            .unwrap();
        assert_eq!(cursor.profile().output_payload_bytes, 0);
        let view = cursor.next_node().unwrap().unwrap();
        assert_eq!(cursor.profile().peak_outstanding_batches, 1);
        assert_eq!(view.column_name(), "node");
        let Some(RetainedNodeField::Value(value)) = view.field("text").unwrap() else {
            panic!("borrowed text");
        };
        assert_eq!(value as *const Value as usize, value_address);
        assert_eq!(value, &expected["text"]);
        assert!(
            matches!(view.field("_id").unwrap(), Some(RetainedNodeField::Identity(found)) if found == id)
        );
        let Some(RetainedNodeField::Labels(labels)) = view.field("labels").unwrap() else {
            panic!("labels");
        };
        assert_eq!(labels.iter().collect::<Vec<_>>(), ["Item"]);
        assert_eq!(
            labels.iter().next().unwrap().as_ptr() as usize,
            label_address
        );
        assert!(view
            .properties()
            .unwrap()
            .all(|(key, _)| key != "_id" && key != "labels"));
        assert!(cursor.next_node().unwrap().is_none());
        assert_eq!(view.status(), RetainedQueryStatus::Completed);
        let second = view.try_retain().unwrap();
        db.query_with_params(
            "MATCH (n:Item {score: $key}) SET n.text = 'changed'",
            &params(),
        )
        .unwrap();
        drop(db);
        assert!(
            matches!(second.field("text").unwrap(), Some(RetainedNodeField::Value(Value::String(text))) if text == "payload")
        );
        drop((cursor, view));
        assert!(governor.retained_result_snapshot().retained_bytes > 0);
        drop(second);
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
        assert_eq!(governor.retained_result_snapshot().view_handles, 0);
    }

    #[test]
    fn node_view_backpressure_does_not_revoke_source_after_cursor_close() {
        let (db, governor) = fixture(2);
        let mut cursor = db
            .query_with_params_retained_node(QUERY, &params(), RetainedQueryOptions::default())
            .unwrap();
        let view = cursor.next_node().unwrap().unwrap();
        let before = governor.retained_result_snapshot().retained_bytes;
        assert!(view.try_retain().unwrap_err().is_retryable());
        assert_eq!(governor.retained_result_snapshot().retained_bytes, before);
        cursor.close();
        assert_eq!(view.status(), RetainedQueryStatus::Closed);
        let second = view.try_retain().unwrap();
        drop((db, cursor, view));
        assert!(second.field("text").unwrap().is_some());
        drop(second);
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    }

    #[test]
    fn full_node_budget_refusal_rolls_back_and_multi_match_never_truncates() {
        let (mut db, governor) = fixture(4);
        let options = RetainedQueryOptions {
            max_result_payload_bytes: Some(0),
            ..Default::default()
        };
        assert!(matches!(
            db.query_with_params_retained_node(QUERY, &params(), options),
            Err(RetainedQueryError::ResultBudget { rows: 1, .. })
        ));
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
        db.query("CREATE (:Item {score: 37})").unwrap();
        assert!(matches!(
            db.query_with_params_retained_node(QUERY, &params(), RetainedQueryOptions::default()),
            Err(RetainedQueryError::UnsupportedPlan)
        ));
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    }

    #[test]
    fn shadowed_source_capacity_is_charged_and_missing_seek_skips_capture() {
        let (mut db, governor) = fixture(4);
        db.query_with_params(
            "MATCH (n:Item {score: $key}) SET n.labels = $large",
            &BTreeMap::from([
                ("key".into(), Value::Int(37)),
                ("large".into(), Value::String("x".repeat(70 * 1024))),
            ]),
        )
        .unwrap();
        let options = RetainedQueryOptions {
            batch_bytes: NonZeroUsize::new(64 * 1024).unwrap(),
            max_result_payload_bytes: Some(1024),
            ..Default::default()
        };
        assert!(matches!(
            db.query_with_params_retained_node(QUERY, &params(), options),
            Err(RetainedQueryError::WorkingUnitTooLarge)
        ));
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
        let mut missing = db
            .query_with_params_retained_node(
                QUERY,
                &BTreeMap::from([("key".into(), Value::Int(999))]),
                RetainedQueryOptions::default(),
            )
            .unwrap();
        assert!(missing.next_node().unwrap().is_none());
        assert_eq!(missing.profile().source_pinned_rows, 0);
        assert_eq!(governor.retained_result_snapshot().retained_bytes, 0);
    }
}

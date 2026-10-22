// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! An explicit ANN window followed by canonical node hydration. Neither a
//! downstream limit nor visibility can change the declared candidate window.

use super::*;
use crate::{ExternalReadResourceContract, ExternalReadResultBudget, VectorIndexExecutionRequest};
use hawdb_storage::read_view::{
    AdmittedNodeRead, AdmittedVec, GraphReadAdmission, GraphReadAllocation,
};

pub(super) fn stream(
    scan: &hawdb_plan_cypher::VectorIndexScan,
    context: BatchReadContext<'_>,
    execution_limit: ExecutionLimit,
    emit: &mut dyn FnMut(BindingBatch) -> Result<BatchControl>,
) -> Result<BatchControl> {
    runtime_checkpoint(context.task_context)?;
    let Some(Value::Vector(embedding)) = context.parameters.get(&scan.embedding_parameter) else {
        return Err(HawDBError::Semantic(
            "vector index requires a typed vector parameter".into(),
        ));
    };
    let resources = ExternalReadResourceContract {
        priority: 0,
        max_parallelism: NonZeroUsize::MIN,
        max_working_memory_bytes: context.memory.blocking_operator_bytes,
        result: ExternalReadResultBudget {
            max_rows: scan.top_k,
            max_memory_bytes: context.memory.blocking_operator_bytes,
        },
        task_context: context.task_context,
    };
    let candidates = context
        .external
        .execute_vector_index(VectorIndexExecutionRequest {
            index: &scan.index,
            embedding: embedding.as_ref(),
            candidate_limit: scan.candidate_limit,
            top_k: scan.top_k,
            resources,
        })?;
    resources.checkpoint()?;
    if candidates.row_count() > scan.top_k {
        return Err(HawDBError::StorageIntegrity(
            "vector index exceeded its declared topK".into(),
        ));
    }
    let account = context
        .kernel_context()
        .source_account("VectorIndexScan canonical nodes");
    let mut allocate =
        |bytes| crate::store::admit_graph_read(&account, context.task_context, bytes);
    let mut allocations =
        AdmittedVec::<Box<dyn GraphReadAllocation>>::new(&GraphReadAdmission::new(&mut allocate))?;
    let mut output = AccountedBindingBatch::with_ledger(
        "VectorIndexScan",
        context.memory.batch_rows.get(),
        context.memory.batch_payload_bytes,
        context.memory_ledger,
    );
    let mut emitted = 0usize;
    for ordinal in 0..candidates.row_count() {
        resources.checkpoint()?;
        if execution_limit.is_reached(emitted) {
            break;
        }
        let candidate = candidates.row(ordinal)?;
        if !candidate.distance.is_finite() || !(0.0..=2.0).contains(&candidate.distance) {
            return Err(HawDBError::StorageIntegrity(
                "vector index returned an invalid cosine distance".into(),
            ));
        }
        let input = context
            .store
            .node_with_allocation(candidate.node_id, None, &mut |bytes| {
                let bytes = bytes
                    .saturating_add(4096)
                    .saturating_add(scan.variable.len())
                    .saturating_add(scan.distance_column.len());
                if !output.is_empty()
                    && (!account.can_reserve(bytes) || output.would_exceed_payload(bytes))
                {
                    let control = output.emit(emit)?;
                    allocations.clear();
                    if control == BatchControl::Stop {
                        return Ok(None);
                    }
                }
                crate::store::admit_graph_read(&account, context.task_context, bytes).map(Some)
            })?;
        let node = match input {
            AdmittedNodeRead::Node(node) => node,
            AdmittedNodeRead::Stopped => return Ok(BatchControl::Stop),
            _ => {
                return Err(HawDBError::StorageIntegrity(
                    "vector index candidate is absent from the bound canonical snapshot".into(),
                ))
            }
        };
        let (node, allocation) = node.into_parts();
        let binding = Binding {
            nodes: BTreeMap::from([(scan.variable.clone(), node)]),
            values: BTreeMap::from([(
                scan.distance_column.clone(),
                Value::Float(candidate.distance),
            )]),
            relationships: BTreeMap::new(),
        };
        if let Some(predicate) = &scan.node_visibility_predicate
            && !evaluate_predicate_observed(
                predicate,
                context.catalog,
                context.store,
                &binding,
                context.observer,
                crate::store::AdjacencyReadMemory {
                    budget_bytes: context.memory.query_memory_bytes.get(),
                    account: Some(&account),
                },
                context.task_context,
            )?
        {
            continue;
        }
        allocations.try_push(allocation)?;
        if output.push(binding, emit)? == BatchControl::Stop {
            return Ok(BatchControl::Stop);
        }
        emitted = emitted.saturating_add(1);
        if output.is_full() {
            let control = output.emit(emit)?;
            allocations.clear();
            if control == BatchControl::Stop {
                return Ok(control);
            }
        }
    }
    resources.checkpoint()?;
    output.emit(emit)
}

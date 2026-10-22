// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;
use hawdb_relational::query::vector_index::{
    execute_vector_index_select, VectorIndexCandidates, VectorIndexInput, VectorIndexSqlExecution,
};
use hawdb_sql::{SqlStatement, SqlValue};

pub struct VectorIndexSqlRequest<'a> {
    pub sql: &'a str,
    pub parameters: &'a [hawdb_core::Value],
    pub output_limits: crate::QueryStreamOptions,
    pub task: RuntimeTaskContext,
}

#[derive(Debug)]
pub struct VectorIndexSqlReport {
    pub output_rows: usize,
    pub output_payload_bytes: usize,
    pub row_execution: hawdb_relational::row_runtime::RelationalRowExecutionEvidence,
    index: Arc<VectorIndexRetainedOutput>,
    _metadata_memory: RuntimeRetainedMemory,
}

impl VectorIndexSqlReport {
    pub fn index(&self) -> &VectorIndexQueryReport {
        self.index.report()
    }
}

impl VectorIndexReadTransaction {
    pub fn query_sql_streaming(
        &self,
        permit: &RuntimePermit,
        budget: VectorIndexLanguageBudget,
        request: VectorIndexSqlRequest<'_>,
        mut consumer: impl FnMut(crate::executor::Row) -> Result<()>,
    ) -> std::result::Result<VectorIndexSqlReport, VectorIndexReadError> {
        self.require_query_permit(permit)?;
        let (index_task, query_task) = budget.split(permit, request.task)?;
        crate::api::query_runtime::query_runtime_checkpoint(Some(&query_task))?;
        let VectorIndexTarget::Relational { table, .. } = self.definition.target() else {
            return Err(
                invalid("SQL vector index search requires a relational column index").into(),
            );
        };
        let metadata_memory = permit
            .reserve_retained_memory(4096)
            .map_err(VectorIndexReadError::Admission)?;
        let prepared = self
            .source
            .relational_plan_template_cache
            .prepare(request.sql)?;
        if prepared.template.parameters.len() != request.parameters.len() {
            return Err(invalid(
                "SQL vector index parameter count differs from the prepared statement",
            )
            .into());
        }
        let SqlStatement::Select(select) = prepared.statement() else {
            return Err(invalid("vector index SQL streaming requires SELECT").into());
        };
        let search = select
            .vector_index
            .as_ref()
            .ok_or_else(|| invalid("SQL source must be vector_index_search"))?;
        let hawdb_core::Value::String(index) = bind(&search.index, request.parameters)? else {
            return Err(invalid("vector index name must be a string").into());
        };
        if !matches!(&search.embedding, SqlValue::Parameter(_)) {
            return Err(invalid("SQL vector query requires a typed vector parameter").into());
        }
        let hawdb_core::Value::Vector(embedding) = bind(&search.embedding, request.parameters)?
        else {
            return Err(invalid("SQL vector query requires a typed vector parameter").into());
        };
        let positive = |value: &SqlValue| -> Result<usize> {
            let hawdb_core::Value::Int(value) = bind(value, request.parameters)? else {
                return Err(invalid("vector index limits must be positive integers"));
            };
            usize::try_from(*value)
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| invalid("vector index limits must be positive integers"))
        };
        let candidate_limit = positive(&search.candidates)?;
        let top_k = positive(&search.top_k)?;
        if top_k > candidate_limit {
            return Err(invalid("vector top_k exceeds candidates").into());
        }
        let mut provider = IndexProvider {
            pin: &self.pin,
            definition: &self.definition,
            permit,
            task: index_task,
            output: None,
            error: None,
        };
        let output = provider.execute_kernel(VectorIndexExecutionRequest {
            index,
            embedding: embedding.as_ref(),
            candidate_limit,
            top_k,
            resources: hawdb_executor::ExternalReadResourceContract {
                priority: 0,
                max_parallelism: NonZeroUsize::MIN,
                max_working_memory_bytes: budget.index_working_bytes,
                result: hawdb_executor::ExternalReadResultBudget {
                    max_rows: top_k,
                    max_memory_bytes: budget.index_result_bytes,
                },
                task_context: Some(&query_task),
            },
        });
        if let Some(error) = provider.error {
            return Err(VectorIndexReadError::Index(error));
        }
        let index = output?;
        let candidates = SqlCandidates(&index);
        let rows = execute_vector_index_select(
            select,
            request.parameters,
            VectorIndexInput {
                table,
                candidates: &candidates,
            },
            VectorIndexSqlExecution {
                state: self.source.store.relational_state(),
                row_mode: hawdb_relational::row_runtime::RelationalRowReadMode::Store(
                    &self.source.store,
                ),
                resources: crate::api::relational_query_resource_context(
                    &self.source.config,
                    request.output_limits.max_rows,
                    request.output_limits.max_payload_bytes,
                    Some(&query_task),
                ),
            },
        )?;
        let report = VectorIndexSqlReport {
            output_rows: rows.row_count(),
            output_payload_bytes: rows.output_payload_bytes,
            row_execution: rows.row_execution.clone(),
            index,
            _metadata_memory: metadata_memory,
        };
        rows.deliver(|row| {
            crate::api::query_runtime::query_runtime_checkpoint(Some(&query_task))?;
            consumer(row)?;
            crate::api::query_runtime::query_runtime_checkpoint(Some(&query_task))
        })?;
        Ok(report)
    }
}

fn bind<'a>(
    value: &'a SqlValue,
    parameters: &'a [hawdb_core::Value],
) -> Result<&'a hawdb_core::Value> {
    match value {
        SqlValue::Literal(value) => Ok(value),
        SqlValue::Parameter(position) => position
            .checked_sub(1)
            .and_then(|position| parameters.get(position))
            .ok_or_else(|| invalid("missing SQL vector index parameter")),
    }
}

struct SqlCandidates<'a>(&'a VectorIndexRetainedOutput);
impl VectorIndexCandidates for SqlCandidates<'_> {
    fn row_count(&self) -> usize {
        self.0.hits().len()
    }
    fn row(&self, ordinal: usize) -> Result<(&str, f64)> {
        let hit = self
            .0
            .hits()
            .get(ordinal)
            .ok_or_else(|| invalid("vector candidate ordinal is out of range"))?;
        Ok((&hit.id, hit.distance))
    }
}

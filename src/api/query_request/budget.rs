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

use super::{QueryRequest, QueryStreamOptions, SqlQueryRequest};
use crate::{
    BoundedReadQueryOutput, DatabaseReadTransaction, HawDBError, QueryOutput, ReadBudgetResource,
    Result,
};

/// Cumulative logical output limits for all statements in one read scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadSnapshotBudget {
    pub max_rows: usize,
    pub max_payload_bytes: usize,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ReadSnapshotUsage {
    pub cypher_statement_count: usize,
    pub sql_statement_count: usize,
    pub output_rows: usize,
    pub output_payload_bytes: usize,
}

/// Owns one immutable read transaction and a single shared Cypher/SQL budget.
/// Failed owned results consume no output quota because none was delivered.
/// The underlying transaction is private so an overload cannot reset the quota.
pub struct BudgetedReadSnapshot<S: crate::executor::ExecutionStore = crate::store::GraphStore> {
    transaction: DatabaseReadTransaction<S>,
    budget: ReadSnapshotBudget,
    usage: ReadSnapshotUsage,
}

impl<S: crate::executor::ExecutionStore> DatabaseReadTransaction<S> {
    pub fn into_bounded(self, budget: ReadSnapshotBudget) -> BudgetedReadSnapshot<S> {
        BudgetedReadSnapshot {
            transaction: self,
            budget,
            usage: ReadSnapshotUsage::default(),
        }
    }
}

impl<S: crate::executor::ExecutionStore> BudgetedReadSnapshot<S> {
    pub fn commit_epoch(&self) -> u64 {
        self.transaction.commit_epoch()
    }
    pub fn usage(&self) -> ReadSnapshotUsage {
        self.usage
    }
    pub fn remaining_limits(&self) -> QueryStreamOptions {
        QueryStreamOptions {
            max_rows: Some(self.budget.max_rows.saturating_sub(self.usage.output_rows)),
            max_payload_bytes: Some(
                self.budget
                    .max_payload_bytes
                    .saturating_sub(self.usage.output_payload_bytes),
            ),
        }
    }
    pub fn query_request(&mut self, request: QueryRequest<'_>) -> Result<QueryOutput> {
        self.query_request_profiled(request)
            .map(|result| result.output)
    }
    pub fn query_request_profiled(
        &mut self,
        request: QueryRequest<'_>,
    ) -> Result<BoundedReadQueryOutput> {
        self.query_request_profiled_with_external(
            request,
            &mut crate::executor::NoExternalReadOperator,
        )
    }
    pub(crate) fn query_request_profiled_with_external(
        &mut self,
        request: QueryRequest<'_>,
        external: &mut dyn crate::executor::ExternalReadOperator,
    ) -> Result<BoundedReadQueryOutput> {
        let count = next_statement_count(self.usage.cypher_statement_count)?;
        let request = request.with_restrictive_output_limits(self.remaining_limits());
        let result = self
            .transaction
            .query_request_profiled_with_external(request, external)?;
        self.consume_output(
            result.output.rows.len(),
            logical_payload_bytes(&result.output),
        )?;
        self.usage.cypher_statement_count = count;
        Ok(result)
    }
    pub(crate) fn consume_output(&mut self, rows: usize, bytes: usize) -> Result<()> {
        let next_rows = self
            .usage
            .output_rows
            .checked_add(rows)
            .filter(|total| *total <= self.budget.max_rows)
            .ok_or_else(|| {
                HawDBError::read_budget_exceeded(
                    ReadBudgetResource::Rows,
                    self.budget.max_rows,
                    "bounded read snapshot exhausted max_rows",
                )
            })?;
        let next_bytes = self
            .usage
            .output_payload_bytes
            .checked_add(bytes)
            .filter(|total| *total <= self.budget.max_payload_bytes)
            .ok_or_else(|| {
                HawDBError::read_budget_exceeded(
                    ReadBudgetResource::PayloadBytes,
                    self.budget.max_payload_bytes,
                    "bounded read snapshot exhausted max_payload_bytes",
                )
            })?;
        self.usage.output_rows = next_rows;
        self.usage.output_payload_bytes = next_bytes;
        Ok(())
    }
}

impl<S: crate::executor::ExecutionStore + hawdb_system_sql::SystemSqlStore>
    BudgetedReadSnapshot<S>
{
    pub fn query_sql_request(&mut self, request: SqlQueryRequest<'_>) -> Result<QueryOutput> {
        let count = next_statement_count(self.usage.sql_statement_count)?;
        let request = request.with_restrictive_output_limits(self.remaining_limits());
        let output = self.transaction.query_sql_request(request)?;
        self.consume_output(output.rows.len(), logical_payload_bytes(&output))?;
        self.usage.sql_statement_count = count;
        Ok(output)
    }
}

fn next_statement_count(count: usize) -> Result<usize> {
    count
        .checked_add(1)
        .ok_or_else(|| HawDBError::Execution("bounded read statement count overflowed".into()))
}

fn logical_payload_bytes(output: &QueryOutput) -> usize {
    if output.rows.is_empty() {
        return 0;
    }
    let names = output
        .rows
        .schema()
        .columns()
        .iter()
        .fold(0usize, |total, name| total.saturating_add(name.len()));
    // Compact results share their schema. Logical delivery charges column names
    // per row, identically for Cypher, SQL, and the Mem snapshot report.
    output
        .rows
        .payload_bytes()
        .saturating_add(names.saturating_mul(output.rows.len() - 1))
}

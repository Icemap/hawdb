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

use crate::{QueryStreamOptions, RelationalJoinPlanningDirective, RuntimeTaskContext, Value};

/// A parameterized PostgreSQL-dialect statement with explicit execution policy.
/// Database/session limits remain upper bounds on the requested output limits.
#[derive(Clone, Copy)]
pub struct SqlQueryRequest<'a> {
    pub(crate) sql: &'a str,
    pub(crate) parameters: &'a [Value],
    pub(crate) task_context: Option<&'a RuntimeTaskContext>,
    pub(crate) output_limits: QueryStreamOptions,
    pub(crate) join_planning: RelationalJoinPlanningDirective,
}

impl<'a> SqlQueryRequest<'a> {
    pub fn new(sql: &'a str) -> Self {
        Self {
            sql,
            parameters: &[],
            task_context: None,
            output_limits: QueryStreamOptions::default(),
            join_planning: RelationalJoinPlanningDirective::Auto,
        }
    }

    pub fn with_params(mut self, parameters: &'a [Value]) -> Self {
        self.parameters = parameters;
        self
    }

    pub fn with_task_context(mut self, context: &'a RuntimeTaskContext) -> Self {
        self.task_context = Some(context);
        self
    }

    pub fn with_output_limits(mut self, limits: QueryStreamOptions) -> Self {
        self.output_limits = limits;
        self
    }

    pub fn with_join_planning(mut self, directive: RelationalJoinPlanningDirective) -> Self {
        self.join_planning = directive;
        self
    }

    pub(in crate::api) fn effective_output_limits(
        &self,
        config: &crate::DatabaseConfig,
    ) -> QueryStreamOptions {
        let result_bytes = self
            .task_context
            .and_then(RuntimeTaskContext::memory_reservation)
            .map(|reservation| usize::try_from(reservation.result_bytes()).unwrap_or(usize::MAX));
        QueryStreamOptions {
            max_rows: super::super::restrictive_query_limit(
                config.read_result_row_limit(),
                self.output_limits.max_rows,
            ),
            max_payload_bytes: super::super::restrictive_query_limit(
                super::super::restrictive_query_limit(
                    config.max_read_result_payload_bytes,
                    self.output_limits.max_payload_bytes,
                ),
                result_bytes,
            ),
        }
    }
}

impl<S> super::super::DatabaseReadTransaction<S>
where
    S: crate::executor::ExecutionStore + hawdb_system_sql::SystemSqlStore,
{
    pub fn query_sql_request(
        &self,
        request: SqlQueryRequest<'_>,
    ) -> crate::Result<crate::QueryOutput> {
        let task = super::bind_request_context(self.task_context.as_ref(), request.task_context)?;
        let request = SqlQueryRequest {
            task_context: task.as_ref(),
            ..request
        };
        let default_context = RuntimeTaskContext::default();
        self.query_sql_with_params_options_context_and_join_planning(
            request.sql,
            request.parameters,
            request.effective_output_limits(&self.config),
            task.as_ref().unwrap_or(&default_context),
            request.join_planning,
        )
    }
}

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

use super::{Database, DatabaseConfig, DatabaseInstanceIdentity};
use crate::relational_sql::{PreparedRelationalSql, RelationalPlanTemplateCache};
use crate::{HawDBError, Result};
use hawdb_qos::{RuntimeRetainedMemory, RuntimeWorkKind, RuntimeWorkRequest};
use std::sync::Arc;

/// Parameter-neutral SQL templates contain no schema bindings or source pins.
pub(crate) struct RuntimeSqlPlanningSnapshot {
    identity: DatabaseInstanceIdentity,
    cache: Arc<RelationalPlanTemplateCache>,
    config: DatabaseConfig,
}

pub(crate) struct PreparedRuntimeSql {
    identity: DatabaseInstanceIdentity,
    config: DatabaseConfig,
    statement: PreparedRelationalSql,
    planning_memory: Option<RuntimeRetainedMemory>,
}

impl RuntimeSqlPlanningSnapshot {
    pub(crate) fn prepare(&self, sql: &str) -> Result<PreparedRuntimeSql> {
        Ok(PreparedRuntimeSql {
            identity: self.identity.clone(),
            config: self.config.clone(),
            statement: self.cache.prepare(sql)?,
            planning_memory: None,
        })
    }
}

impl PreparedRuntimeSql {
    pub(crate) fn with_planning_memory(mut self, memory: RuntimeRetainedMemory) -> Self {
        self.planning_memory = Some(memory);
        self
    }
    pub(crate) fn planning_memory_bytes(&self) -> u64 {
        self.planning_memory
            .as_ref()
            .map_or(0, RuntimeRetainedMemory::bytes)
    }
    pub(crate) fn uses_snapshot(&self) -> bool {
        super::concurrent::sql_statement_uses_snapshot(self.statement.statement())
    }
    pub(crate) fn work_request(&self, result_bytes: u64) -> RuntimeWorkRequest {
        // SQL binds access paths to the execution snapshot. Admit the configured
        // working ceiling until relational physical preparation exposes a shared
        // estimator; never inspect a source snapshot while queued for capacity.
        let memory = u64::try_from(self.config.execution_memory.query_memory_bytes.get())
            .unwrap_or(u64::MAX);
        RuntimeWorkRequest::foreground_query(memory, result_bytes)
            .with_kind(if self.uses_snapshot() {
                RuntimeWorkKind::Query
            } else {
                RuntimeWorkKind::Mutation
            })
            .with_io_wave_slots(1)
    }
    pub(crate) fn into_statement(
        self,
        database: &Database,
    ) -> Result<(PreparedRelationalSql, Option<RuntimeRetainedMemory>)> {
        if self.identity != database.runtime.instance_identity() || self.config != database.config {
            return Err(HawDBError::Execution(
                "SQL execution context changed during admission; retry the query".into(),
            ));
        }
        Ok((self.statement, self.planning_memory))
    }
}

impl Database {
    pub(crate) fn runtime_sql_planning_snapshot(&self) -> RuntimeSqlPlanningSnapshot {
        RuntimeSqlPlanningSnapshot {
            identity: self.runtime.instance_identity(),
            cache: self.runtime.peek().map_or_else(
                || Arc::new(RelationalPlanTemplateCache::new(Some(0))),
                |runtime| Arc::clone(&runtime.relational_plan_template_cache),
            ),
            config: self.config.clone(),
        }
    }
}

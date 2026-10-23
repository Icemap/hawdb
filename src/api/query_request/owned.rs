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

use super::{
    QueryAccessControlContext, QueryRequest, QueryStreamOptions, ScoringAttachment, ScoringRequest,
    SqlQueryRequest,
};
use crate::{RelationalJoinPlanningDirective, RuntimeTaskContext, Value};
use std::collections::BTreeMap;

mod host;
pub use host::OwnedHostScoringRequest;

enum OwnedScoringAttachment {
    Program(ScoringRequest),
    Host(OwnedHostScoringRequest),
}

/// Owns the same Cypher request contract across an asynchronous admission wait.
/// It contains caller input and callback ownership, never a database snapshot.
pub struct OwnedQueryRequest {
    cypher: String,
    parameters: BTreeMap<String, Value>,
    scoring: Option<OwnedScoringAttachment>,
    access_control: Option<QueryAccessControlContext>,
    task_context: Option<RuntimeTaskContext>,
    output_limits: QueryStreamOptions,
}

impl OwnedQueryRequest {
    pub fn new(cypher: impl Into<String>) -> Self {
        Self {
            cypher: cypher.into(),
            parameters: BTreeMap::new(),
            scoring: None,
            access_control: None,
            task_context: None,
            output_limits: QueryStreamOptions::default(),
        }
    }

    pub fn with_params(mut self, parameters: BTreeMap<String, Value>) -> Self {
        self.parameters = parameters;
        self
    }

    pub fn with_scoring(mut self, scoring: ScoringRequest) -> Self {
        self.scoring = Some(OwnedScoringAttachment::Program(scoring));
        self
    }

    pub fn with_host_scoring(mut self, scoring: OwnedHostScoringRequest) -> Self {
        self.scoring = Some(OwnedScoringAttachment::Host(scoring));
        self
    }

    pub fn with_access_control(mut self, access: QueryAccessControlContext) -> Self {
        self.access_control = Some(access);
        self
    }

    pub fn with_task_context(mut self, context: RuntimeTaskContext) -> Self {
        self.task_context = Some(context);
        self
    }

    pub fn with_output_limits(mut self, limits: QueryStreamOptions) -> Self {
        self.output_limits = limits;
        self
    }

    /// Borrows all attachments for the ordinary synchronous execution kernel.
    pub fn as_request(&self) -> QueryRequest<'_> {
        QueryRequest {
            cypher: &self.cypher,
            parameters: Some(&self.parameters),
            scoring: self.scoring.as_ref().map(|attachment| match attachment {
                OwnedScoringAttachment::Program(program) => ScoringAttachment::Program(program),
                OwnedScoringAttachment::Host(host) => host.attachment(),
            }),
            access_control: self.access_control.as_ref(),
            task_context: self.task_context.as_ref(),
            output_limits: self.output_limits,
        }
    }
}

/// Owns the SQL statement and its request constraints across asynchronous work.
#[derive(Debug, Clone)]
pub struct OwnedSqlQueryRequest {
    sql: String,
    parameters: Vec<Value>,
    task_context: Option<RuntimeTaskContext>,
    output_limits: QueryStreamOptions,
    join_planning: RelationalJoinPlanningDirective,
}

impl OwnedSqlQueryRequest {
    pub fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            parameters: Vec::new(),
            task_context: None,
            output_limits: QueryStreamOptions::default(),
            join_planning: RelationalJoinPlanningDirective::Auto,
        }
    }
    pub fn with_params(mut self, parameters: Vec<Value>) -> Self {
        self.parameters = parameters;
        self
    }
    pub fn with_task_context(mut self, context: RuntimeTaskContext) -> Self {
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
    pub fn as_request(&self) -> SqlQueryRequest<'_> {
        SqlQueryRequest {
            sql: &self.sql,
            parameters: &self.parameters,
            task_context: self.task_context.as_ref(),
            output_limits: self.output_limits,
            join_planning: self.join_planning,
        }
    }
}

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

use super::*;
use hawdb_resource_profile::StorageResourceProfileObservation;

pub use hawdb_resource_profile::{
    StorageResourceProfileLimits, StorageResourceProfileReport, STORAGE_RESOURCE_PROFILE_PROTOCOL,
};

impl Database {
    pub fn storage_resource_profile(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        limits: StorageResourceProfileLimits,
    ) -> Result<StorageResourceProfileReport> {
        limits.validate()?;
        self.begin_read_transaction()?
            .storage_resource_profile_with_binding(
                || self.prepare_runtime_query(cypher_text.to_string(), parameters),
                parameters,
                limits,
                None,
                &hawdb_core::RuntimeTaskContext::default(),
            )
    }

    pub fn storage_resource_profile_for_production(
        &self,
        cypher_text: &str,
        parameters: &BTreeMap<String, Value>,
        limits: StorageResourceProfileLimits,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
    ) -> Result<StorageResourceProfileReport> {
        limits.validate()?;
        self.begin_read_transaction()?
            .storage_resource_profile_with_binding(
                || self.prepare_runtime_query(cypher_text.to_string(), parameters),
                parameters,
                limits,
                Some((evidence_binding, expected_identity)),
                &hawdb_core::RuntimeTaskContext::default(),
            )
    }
}

impl DatabaseReadTransaction {
    pub(crate) fn storage_resource_profile_for_production_with_context(
        &mut self,
        prepared: super::query_runtime::PreparedRuntimeQuery,
        parameters: &BTreeMap<String, Value>,
        limits: StorageResourceProfileLimits,
        evidence_binding: crate::ProductionEvidenceBinding,
        expected_identity: crate::ProductionQualificationIdentity,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<StorageResourceProfileReport> {
        self.storage_resource_profile_with_binding(
            || Ok(prepared),
            parameters,
            limits,
            Some((evidence_binding, expected_identity)),
            task_context,
        )
    }

    fn storage_resource_profile_with_binding(
        &mut self,
        prepare: impl FnOnce() -> Result<super::query_runtime::PreparedRuntimeQuery>,
        parameters: &BTreeMap<String, Value>,
        limits: StorageResourceProfileLimits,
        production: Option<(
            crate::ProductionEvidenceBinding,
            crate::ProductionQualificationIdentity,
        )>,
        task_context: &hawdb_core::RuntimeTaskContext,
    ) -> Result<StorageResourceProfileReport> {
        limits.validate()?;
        let canonical_graph_commit_epoch = self.commit_epoch();
        let (evidence_binding, expected_identity) = if let Some((binding, expected)) = production {
            binding.validate_for(&expected)?;
            if binding.identity.canonical_graph_commit_epoch != canonical_graph_commit_epoch {
                return Err(HawDBError::Semantic(format!(
                    "production evidence canonical graph commit epoch {} does not match database epoch {canonical_graph_commit_epoch}",
                    binding.identity.canonical_graph_commit_epoch
                )));
            }
            (Some(binding), Some(expected))
        } else {
            (None, None)
        };
        let durable = self.store.storage_recovery_report().durable;
        let before = self.store.storage_residency_report();
        let stream_options = QueryStreamOptions {
            max_rows: Some(limits.max_output_rows),
            max_payload_bytes: Some(limits.max_output_payload_bytes),
        };
        // Raw callers measure preparation after the residency baseline; admitted
        // callers hand over the already-accounted preparation without repeating it.
        let query = self.query_prepared_with_params_streaming_context(
            prepare()?,
            parameters,
            stream_options,
            executor::StreamDelivery::Validated,
            task_context,
            |_| Ok(()),
        )?;
        let after = self.store.storage_residency_report();

        Ok(StorageResourceProfileReport::from_observation(
            StorageResourceProfileObservation {
                canonical_graph_commit_epoch,
                limits,
                durable,
                before,
                after,
                query,
                evidence_binding,
                expected_identity,
            },
        ))
    }
}

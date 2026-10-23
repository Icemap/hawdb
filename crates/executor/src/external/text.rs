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

use super::{ExternalReadResourceContract, ExternalReadResultBudget};
use crate::{QueryMemoryAccount, QueryMemoryLease};
use hawdb_core::{HawDBError, Result};
use std::collections::BTreeMap;
use std::mem::size_of;

pub struct TextSeedExecutionRequest<'a> {
    pub query_text: &'a str,
    pub metadata_filters: &'a BTreeMap<String, String>,
    pub resources: ExternalReadResourceContract<'a>,
    pub working_account: &'a QueryMemoryAccount,
    pub result_account: &'a QueryMemoryAccount,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{QueryMemoryClass, QueryMemoryLedger};
    use std::num::NonZeroUsize;

    #[test]
    fn text_result_owner_retains_the_query_root_allowance_until_drop() {
        for growing in [false, true] {
            let bytes = NonZeroUsize::new(1024).unwrap();
            let ledger = QueryMemoryLedger::new(bytes);
            let parent = ledger.account(QueryMemoryClass::ExternalRead, "text allowance", bytes);
            let other = ledger.account(QueryMemoryClass::PipelineBatch, "downstream", bytes);
            let admitted = parent.sub_account(bytes).unwrap();
            let account = admitted.sibling(QueryMemoryClass::ExternalRead, "text result", bytes);
            let budget = ExternalReadResultBudget {
                max_rows: 2,
                max_memory_bytes: bytes,
            };
            let mut output = if growing {
                TextSeedExecutionOutput::with_initial_row_capacity(&account, budget, 0)
            } else {
                TextSeedExecutionOutput::new(&account, budget)
            }
            .unwrap();
            output
                .push("document:first", Some("canonical:first"), 1.0)
                .unwrap();
            output
                .push("document:second", Some("canonical:second"), 2.0)
                .unwrap();
            output.validate(&account, budget).unwrap();
            drop(account);
            drop(admitted);
            assert_eq!(ledger.snapshot().used_bytes, bytes.get());
            assert!(
                other.reserve(1).is_err(),
                "external output detached its root allowance"
            );
            assert_eq!(
                output.rows()[1].external_id.as_deref(),
                Some("canonical:second")
            );
            drop(output);
            assert_eq!(ledger.snapshot().used_bytes, 0);
            assert!(other.reserve(bytes.get()).is_ok());
        }
    }

    #[test]
    fn text_result_builder_refuses_row_payload_and_nonfinite_failures_without_partial_success() {
        for failure in ["row", "payload", "score"] {
            let bytes = NonZeroUsize::new(256).unwrap();
            let ledger = QueryMemoryLedger::new(bytes);
            let account = ledger.account(QueryMemoryClass::ExternalRead, "text result", bytes);
            let budget = ExternalReadResultBudget {
                max_rows: if failure == "row" { 1 } else { 2 },
                max_memory_bytes: bytes,
            };
            let mut output = TextSeedExecutionOutput::new(&account, budget).unwrap();
            output.push("a", Some("canonical:a"), 1.0).unwrap();
            let before = ledger.snapshot().used_bytes;
            let oversized = "x".repeat(1024);
            let (id, score) = match failure {
                "row" => ("b", 2.0),
                "payload" => (oversized.as_str(), 2.0),
                _ => ("b", f64::INFINITY),
            };
            let error = output.push(id, Some("canonical:b"), score).unwrap_err();
            if failure == "row" {
                assert!(matches!(&error, HawDBError::ReadBudgetExceeded(cause)
                    if cause.resource == hawdb_core::ReadBudgetResource::Rows && cause.limit == 1));
            } else {
                assert!(matches!(&error, HawDBError::Execution(_)));
            }
            assert_eq!(
                ledger.snapshot().used_bytes,
                before,
                "{failure} charged a rejected copy"
            );
            assert_eq!(output.rows().len(), 1);
            assert_eq!(output.validate(&account, budget).unwrap_err(), error);
            assert_eq!(output.push("c", None, 3.0).unwrap_err(), error);
            assert_eq!(ledger.snapshot().used_bytes, before);
            drop(output);
            assert_eq!(ledger.snapshot().used_bytes, 0);
            assert!(account.reserve(bytes.get()).is_ok());
        }
    }

    #[test]
    fn text_result_owner_requires_the_exact_admitted_account_and_prepaid_row_storage() {
        let bytes = NonZeroUsize::new(256).unwrap();
        let ledger = QueryMemoryLedger::new(bytes);
        let account = ledger.account(QueryMemoryClass::ExternalRead, "text result", bytes);
        let foreign_ledger = QueryMemoryLedger::new(bytes);
        let foreign = foreign_ledger.account(QueryMemoryClass::ExternalRead, "foreign", bytes);
        let sibling = ledger.account(QueryMemoryClass::ExternalRead, "wrong sibling", bytes);
        let budget = ExternalReadResultBudget {
            max_rows: 1,
            max_memory_bytes: bytes,
        };
        for wrong in [&foreign, &sibling] {
            let output = TextSeedExecutionOutput::new(wrong, budget).unwrap();
            assert!(output.validate(&account, budget).is_err());
        }
        let mut output = TextSeedExecutionOutput::new(&account, budget).unwrap();
        output.push("a", None, 1.0).unwrap();
        let before = ledger.snapshot().used_bytes;
        let narrowed = ExternalReadResultBudget {
            max_rows: 0,
            ..budget
        };
        assert!(matches!(output.validate(&account, narrowed),
            Err(HawDBError::ReadBudgetExceeded(cause))
                if cause.resource == hawdb_core::ReadBudgetResource::Rows && cause.limit == 0));
        assert_eq!(output.rows().len(), 1);
        assert_eq!(ledger.snapshot().used_bytes, before);
        drop(output);
        assert_eq!(ledger.snapshot().used_bytes, 0);
        let held = account.reserve(bytes.get()).unwrap();
        assert!(TextSeedExecutionOutput::new(&account, budget).is_err());
        assert_eq!(ledger.snapshot().used_bytes, bytes.get());
        drop(held);
        let oversized_rows = ExternalReadResultBudget {
            max_rows: usize::MAX,
            ..budget
        };
        assert!(TextSeedExecutionOutput::new(&account, oversized_rows).is_err());
        assert_eq!(ledger.snapshot().used_bytes, 0);
        assert!(TextSeedExecutionOutput::new(&account, budget).is_ok());
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }

    #[test]
    fn growing_text_result_uses_actual_capacity_and_admits_replacement_overlap() {
        let row_bytes = size_of::<TextSeedExecutionRow>();
        let bytes = NonZeroUsize::new(6 * row_bytes + 3).unwrap();
        let ledger = QueryMemoryLedger::new(bytes);
        let account = ledger.account(QueryMemoryClass::ExternalRead, "growing text", bytes);
        let budget = ExternalReadResultBudget {
            max_rows: 1_000_000,
            max_memory_bytes: bytes,
        };
        let mut output =
            TextSeedExecutionOutput::with_initial_row_capacity(&account, budget, 0).unwrap();
        assert_eq!(output.rows.capacity(), 0);
        assert_eq!(ledger.snapshot().used_bytes, 0);
        for (id, score) in [("a", 3.0), ("b", 2.0), ("c", 1.0)] {
            output.push(id, None, score).unwrap();
        }
        output.validate(&account, budget).unwrap();
        assert_eq!(output.rows.capacity(), 4);
        assert_eq!(output.rows()[2].id, "c");
        assert_eq!(ledger.snapshot().used_bytes, 4 * row_bytes + 3);
        assert_eq!(ledger.snapshot().peak_bytes, 6 * row_bytes + 3);
        drop(output);
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }

    #[test]
    fn growing_text_result_refuses_before_replacement_and_keeps_failure_sticky() {
        let row_bytes = size_of::<TextSeedExecutionRow>();
        let bytes = NonZeroUsize::new(6 * row_bytes + 2).unwrap();
        let ledger = QueryMemoryLedger::new(bytes);
        let account = ledger.account(QueryMemoryClass::ExternalRead, "growing text", bytes);
        let budget = ExternalReadResultBudget {
            max_rows: 1_000_000,
            max_memory_bytes: bytes,
        };
        let mut output =
            TextSeedExecutionOutput::with_initial_row_capacity(&account, budget, 0).unwrap();
        output.push("a", None, 3.0).unwrap();
        output.push("b", None, 2.0).unwrap();
        let before = ledger.snapshot();
        let error = output.push("c", None, 1.0).unwrap_err();
        assert!(matches!(&error, HawDBError::ResourceBudgetExceeded(cause)
            if cause.resource == hawdb_core::ResourceBudgetKind::MemoryBytes
                && cause.scope == hawdb_core::ResourceBudgetScope::Operator
                && cause.requested == (4 * row_bytes + 1) as u64
                && cause.used == (2 * row_bytes + 2) as u64
                && cause.budget == bytes.get() as u64));
        assert_eq!(output.rows.capacity(), 2);
        assert_eq!(output.rows().len(), 2);
        assert_eq!(ledger.snapshot().used_bytes, before.used_bytes);
        assert_eq!(ledger.snapshot().peak_bytes, before.peak_bytes);
        assert_eq!(output.validate(&account, budget).unwrap_err(), error);
        assert_eq!(output.push("d", None, 0.0).unwrap_err(), error);
        drop(output);
        assert_eq!(ledger.snapshot().used_bytes, 0);
    }

    #[test]
    fn growing_text_result_clamps_capacity_and_refuses_an_incomplete_prefix() {
        let bytes = NonZeroUsize::new(1024).unwrap();
        for capacity in 0..=3 {
            let ledger = QueryMemoryLedger::new(bytes);
            let account = ledger.account(QueryMemoryClass::ExternalRead, "growing text", bytes);
            let budget = ExternalReadResultBudget {
                max_rows: 3,
                max_memory_bytes: bytes,
            };
            let mut output =
                TextSeedExecutionOutput::with_initial_row_capacity(&account, budget, capacity)
                    .unwrap();
            for id in ["a", "b", "c"] {
                output.push(id, None, 1.0).unwrap();
            }
            assert_eq!(output.rows.capacity(), 3);
            assert_eq!(
                output
                    .rows()
                    .iter()
                    .map(|row| row.id.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b", "c"]
            );
            let before = ledger.snapshot().used_bytes;
            let error = output.push("d", None, 1.0).unwrap_err();
            assert!(matches!(&error, HawDBError::ReadBudgetExceeded(cause)
                if cause.resource == hawdb_core::ReadBudgetResource::Rows && cause.limit == 3));
            assert_eq!(output.rows().len(), 3);
            assert_eq!(output.rows.capacity(), 3);
            assert_eq!(ledger.snapshot().used_bytes, before);
            assert_eq!(output.validate(&account, budget).unwrap_err(), error);
            drop(output);
            assert_eq!(ledger.snapshot().used_bytes, 0);
        }
    }
}

#[derive(Debug)]
pub struct TextSeedExecutionRow {
    pub id: String,
    pub external_id: Option<String>,
    pub score: f64,
}

/// Rows admitted before copying and retained in the supplied result account.
/// The caller can borrow rows, but cannot detach their owning reservation.
#[derive(Debug)]
pub struct TextSeedExecutionOutput {
    rows: Vec<TextSeedExecutionRow>,
    budget: ExternalReadResultBudget,
    prepaid_rows: bool,
    exhausted: bool,
    source_report: Option<crate::TextSeedSourceReport>,
    failure: Option<HawDBError>,
    reservation: QueryMemoryLease,
}

impl TextSeedExecutionOutput {
    pub fn new(account: &QueryMemoryAccount, budget: ExternalReadResultBudget) -> Result<Self> {
        Self::with_initial_row_capacity(account, budget, budget.max_rows)
    }

    /// Construct a complete-domain builder without prepaying an unused ceiling.
    /// The provider calls finish_complete only after exhausting its source.
    pub fn new_complete(
        account: &QueryMemoryAccount,
        budget: ExternalReadResultBudget,
    ) -> Result<Self> {
        Self::with_initial_row_capacity(account, budget, 0)
    }

    pub fn finish_complete(mut self) -> Result<Self> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        self.exhausted = true;
        Ok(self)
    }

    pub fn is_complete(&self) -> bool {
        self.exhausted
    }

    pub fn with_source_report(mut self, report: crate::TextSeedSourceReport) -> Self {
        self.source_report = Some(report);
        self
    }

    pub fn source_report(&self) -> Option<crate::TextSeedSourceReport> {
        self.source_report
    }

    pub fn retained_bytes(&self) -> usize {
        self.reservation.bytes()
    }

    fn with_initial_row_capacity(
        account: &QueryMemoryAccount,
        budget: ExternalReadResultBudget,
        capacity: usize,
    ) -> Result<Self> {
        if capacity > budget.max_rows {
            return Err(HawDBError::Execution(
                "text seed initial capacity exceeds its row budget".into(),
            ));
        }
        let bytes = capacity
            .checked_mul(size_of::<TextSeedExecutionRow>())
            .ok_or_else(|| {
                HawDBError::Execution("text seed result storage size overflow".into())
            })?;
        if bytes > budget.max_memory_bytes.get() {
            return Err(HawDBError::Execution(
                "text seed result row storage exceeds its memory budget".into(),
            ));
        }
        let reservation = account.reserve(bytes)?;
        let rows = Vec::with_capacity(capacity);
        Ok(Self {
            rows,
            budget,
            prepaid_rows: capacity == budget.max_rows,
            exhausted: false,
            source_report: None,
            failure: None,
            reservation,
        })
    }

    pub fn push(&mut self, id: &str, external_id: Option<&str>, score: f64) -> Result<()> {
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        let result = (|| {
            if self.exhausted {
                return Err(HawDBError::Execution(
                    "text seed source was already exhausted".into(),
                ));
            }
            if !score.is_finite() {
                return Err(HawDBError::Execution(
                    "text seed score must be finite".into(),
                ));
            }
            if self.rows.len() >= self.budget.max_rows {
                return Err(HawDBError::read_budget_exceeded(
                    hawdb_core::ReadBudgetResource::Rows,
                    self.budget.max_rows,
                    "text seed result exceeds its row budget",
                ));
            }
            let bytes = id
                .len()
                .checked_add(external_id.map_or(0, str::len))
                .ok_or_else(|| {
                    HawDBError::Execution("text seed result payload size overflow".into())
                })?;
            let grow_to = (self.rows.len() == self.rows.capacity()).then(|| {
                self.rows
                    .capacity()
                    .saturating_mul(2)
                    .max(1)
                    .min(self.budget.max_rows)
            });
            let replacement_bytes = grow_to
                .map(|capacity| capacity.checked_mul(size_of::<TextSeedExecutionRow>()))
                .unwrap_or(Some(0))
                .ok_or_else(|| {
                    HawDBError::Execution("text seed result storage size overflow".into())
                })?;
            let requested = bytes.checked_add(replacement_bytes).ok_or_else(|| {
                HawDBError::Execution("text seed result payload size overflow".into())
            })?;
            let next = self
                .reservation
                .bytes()
                .checked_add(requested)
                .ok_or_else(|| {
                    HawDBError::Execution("text seed result payload size overflow".into())
                })?;
            if next > self.budget.max_memory_bytes.get() {
                return Err(if !self.prepaid_rows {
                    self.reservation.operator_budget_error(
                        requested,
                        self.reservation.bytes(),
                        self.budget.max_memory_bytes.get(),
                        "ExternalReadResultBudget.max_memory_bytes",
                        "text seed result growth exceeds its memory budget".into(),
                    )
                } else {
                    HawDBError::Execution(
                        "text seed result payload exceeds its memory budget".into(),
                    )
                });
            }
            self.reservation.grow(requested)?;
            if let Some(capacity) = grow_to {
                // Admit old and new storage together before reallocating; move
                // payload owners without copying and release the old slots last.
                let old_bytes = self.rows.capacity() * size_of::<TextSeedExecutionRow>();
                let mut rows = Vec::with_capacity(capacity);
                rows.append(&mut self.rows);
                self.rows = rows;
                self.reservation.shrink(old_bytes);
            }
            self.rows.push(TextSeedExecutionRow {
                id: id.into(),
                external_id: external_id.map(str::to_owned),
                score,
            });
            Ok(())
        })();
        if let Err(error) = &result {
            self.failure = Some(error.clone());
        }
        result
    }

    pub fn rows(&self) -> &[TextSeedExecutionRow] {
        &self.rows
    }

    pub(crate) fn validate(
        &self,
        account: &QueryMemoryAccount,
        budget: ExternalReadResultBudget,
    ) -> Result<()> {
        if !self.reservation.belongs_to(account) {
            return Err(HawDBError::Execution(
                "text seed output did not retain the admitted result account and budget".into(),
            ));
        }
        if let Some(error) = &self.failure {
            return Err(error.clone());
        }
        if self.rows.len() > budget.max_rows {
            return Err(HawDBError::read_budget_exceeded(
                hawdb_core::ReadBudgetResource::Rows,
                budget.max_rows,
                "text seed result exceeds its validated row budget",
            ));
        }
        if self.reservation.bytes() > budget.max_memory_bytes.get() {
            return Err(HawDBError::Execution(
                "text seed output did not retain the admitted result account and budget".into(),
            ));
        }
        Ok(())
    }
}

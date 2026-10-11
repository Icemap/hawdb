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

//! Capacity admission and ownership transfer for compact owned query results.

use super::{QueryRowStorage, QueryRowsBuilder, QuerySchema, Row, OWNED_RESULT_SEGMENT_ROWS};
use crate::{QueryMemoryAccount, QueryMemoryLease};
use hawdb_core::{HawDBError, Result, Value};
use std::sync::Arc;

impl QueryRowsBuilder {
    pub(crate) fn owner_memory_bytes() -> usize {
        std::mem::size_of::<QueryRowStorage>() + 4 * std::mem::size_of::<usize>()
    }

    pub(crate) fn push_budgeted_named_row(
        &mut self,
        row: Row,
        account: &QueryMemoryAccount,
        retained: &mut QueryMemoryLease,
    ) -> Result<()> {
        if let Some(schema) = &self.schema
            && (row.len() != schema.len() || !row.keys().eq(schema.columns().iter()))
        {
            return Err(HawDBError::Execution(
                "query result row does not match its bound schema".into(),
            ));
        }
        let next_count = self
            .row_count
            .checked_add(1)
            .ok_or_else(result_capacity_overflow)?;
        let heap_bytes = row.values().try_fold(0usize, |total, value| {
            checked_result_add(total, retained_value_heap_bytes(value)?)
        })?;
        // The incoming map remains live while its payload is moved into the
        // result. Keep its existing opaque-map node estimate plus capacity
        // accounting until the map and its keys have actually been destroyed.
        let key_spare_bytes = row.keys().try_fold(0usize, |total, name| {
            checked_result_add(total, name.capacity().saturating_sub(name.len()))
        })?;
        let transient = account.reserve(checked_result_add(
            crate::binding::map_memory_bytes(&row),
            checked_result_add(heap_bytes, key_spare_bytes)?,
        )?)?;
        if self.schema.is_none() {
            let slots = checked_result_mul(row.len(), std::mem::size_of::<String>())?;
            let names = row
                .keys()
                .try_fold(0usize, |total, name| checked_result_add(total, name.len()))?;
            // The schema Arc header is pre-admitted with the result owner,
            // including queries that emit no rows.
            let schema_bytes = checked_result_add(slots, names)?;
            let staging = account.reserve(slots)?;
            retained.grow(schema_bytes)?;
            let columns = row.keys().cloned().collect::<Vec<_>>();
            // BTreeMap already proves uniqueness and deterministic order;
            // avoid a second temporary tree solely to recheck the schema.
            self.schema = Some(QuerySchema {
                columns: Arc::from(columns),
            });
            drop(staging);
        }
        let width = row.len();
        let rows_in_segment = self.row_count % OWNED_RESULT_SEGMENT_ROWS;
        let starts_segment = width != 0 && rows_in_segment == 0 && self.row_count != 0;
        let mut next_segment = Vec::new();
        if width != 0 {
            let segments = self
                .segments
                .as_mut()
                .expect("budgeted results use segmented storage");
            let needed = checked_result_add(self.row_count / OWNED_RESULT_SEGMENT_ROWS, 1)?;
            // A single active segment needs no directory in the final result.
            if (starts_segment || !segments.is_empty()) && needed > segments.capacity() {
                let target = needed
                    .checked_next_power_of_two()
                    .ok_or_else(result_capacity_overflow)?;
                reserve_result_capacity(segments, target, retained)?;
            }
            if starts_segment {
                reserve_result_capacity(&mut next_segment, width, retained)?;
            } else {
                let needed_values = checked_result_mul(rows_in_segment + 1, width)?;
                if needed_values > self.values.capacity() {
                    let target =
                        checked_result_mul((rows_in_segment + 1).next_power_of_two(), width)?;
                    reserve_result_capacity(&mut self.values, target, retained)?;
                }
            }
        }
        if let Err(error) = retained.grow(heap_bytes) {
            let next_bytes =
                checked_result_mul(next_segment.capacity(), std::mem::size_of::<Value>())?;
            drop(next_segment);
            retained.shrink(next_bytes);
            return Err(error);
        }
        if starts_segment {
            self.segments
                .as_mut()
                .expect("budgeted results use segmented storage")
                .push(std::mem::replace(&mut self.values, next_segment));
        }
        self.values.extend(row.into_values());
        self.row_count = next_count;
        drop(transient);
        Ok(())
    }
}

fn result_capacity_overflow() -> HawDBError {
    HawDBError::Execution("query result retained capacity overflow".into())
}

fn checked_result_add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right).ok_or_else(result_capacity_overflow)
}

fn checked_result_mul(left: usize, right: usize) -> Result<usize> {
    left.checked_mul(right).ok_or_else(result_capacity_overflow)
}

fn retained_value_heap_bytes(value: &Value) -> Result<usize> {
    match value {
        Value::String(value) => Ok(value.capacity()),
        Value::Binary(value) => Ok(value.capacity()),
        Value::List(values) => values.iter().try_fold(
            checked_result_mul(values.capacity(), std::mem::size_of::<Value>())?,
            |total, value| checked_result_add(total, retained_value_heap_bytes(value)?),
        ),
        Value::Map(values) => {
            // std does not expose BTreeMap node capacity. Preserve the existing
            // opaque-map entry estimate, with capacities for retained payloads.
            values
                .iter()
                .try_fold(std::mem::size_of::<Row>(), |total, (name, value)| {
                    let entry = checked_result_mul(3, std::mem::size_of::<(String, Value)>())?;
                    checked_result_add(
                        total,
                        checked_result_add(
                            entry,
                            checked_result_add(name.capacity(), retained_value_heap_bytes(value)?)?,
                        )?,
                    )
                })
        }
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Uuid(_) => Ok(0),
    }
}

fn reserve_result_capacity<T>(
    values: &mut Vec<T>,
    target: usize,
    retained: &mut QueryMemoryLease,
) -> Result<()> {
    let old_bytes = checked_result_mul(values.capacity(), std::mem::size_of::<T>())?;
    let new_bytes = checked_result_mul(target, std::mem::size_of::<T>())?;
    // Admit the full replacement while the old buffer is still retained.
    // This explicitly accounts for growth overlap rather than relying on realloc.
    retained.grow(new_bytes)?;
    let mut replacement = Vec::new();
    if let Err(error) = replacement.try_reserve_exact(target) {
        retained.shrink(new_bytes);
        return Err(HawDBError::Execution(format!(
            "query result allocation failed: {error}"
        )));
    }
    let actual_bytes = checked_result_mul(replacement.capacity(), std::mem::size_of::<T>())?;
    if actual_bytes > new_bytes {
        if let Err(error) = retained.grow(actual_bytes - new_bytes) {
            drop(replacement);
            retained.shrink(new_bytes);
            return Err(error);
        }
    } else {
        retained.shrink(new_bytes - actual_bytes);
    }
    let old = std::mem::replace(values, replacement);
    values.extend(old);
    retained.shrink(old_bytes);
    Ok(())
}

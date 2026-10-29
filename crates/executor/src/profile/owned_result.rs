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

use super::{
    QueryRowStorage, QueryRowsBuilder, QuerySchema, Row, RowRef, OWNED_RESULT_SEGMENT_ROWS,
};
use crate::{QueryMemoryAccount, QueryMemoryLease};
use hawdb_core::{HawDBError, Result, Value, ValueRef};
use std::sync::Arc;

impl QueryRowsBuilder {
    pub(crate) fn owner_memory_bytes() -> usize {
        // Two Arc reference-count pairs cover the result storage and schema columns.
        std::mem::size_of::<QueryRowStorage>() + 4 * std::mem::size_of::<usize>()
    }

    pub(crate) fn push_budgeted_named_row(
        &mut self,
        mut row: Row,
        account: &QueryMemoryAccount,
        retained: &mut QueryMemoryLease,
    ) -> Result<()> {
        let heap_bytes = row.values().try_fold(0usize, |total, value| {
            checked_result_add(total, retained_value_heap_bytes(value)?)
        })?;
        // Keep the incoming tree, keys and payload charged until ownership
        // transfer destroys the tree. Payload accounting is shared with the
        // retained result instead of traversing it again as a logical estimate.
        let key_bytes = row.keys().try_fold(0usize, |total, name| {
            checked_result_add(total, name.capacity())
        })?;
        let transient = account.reserve(checked_result_add(
            checked_result_add(
                std::mem::size_of::<Row>(),
                retained_map_node_bound(row.len())?,
            )?,
            checked_result_add(heap_bytes, key_bytes)?,
        )?)?;
        let next_count = self.prepare_budgeted_row(
            row.keys().map(String::as_str),
            row.len(),
            heap_bytes,
            account,
            retained,
        )?;
        let schema = self.schema.as_ref().expect("result schema is bound");
        if row.keys().eq(schema.columns().iter()) {
            self.values.extend(row.into_values());
        } else {
            self.values.extend(schema.columns().iter().map(|name| {
                row.remove(name)
                    .expect("row keys were validated before transfer")
            }));
        }
        self.row_count = next_count;
        drop(transient);
        Ok(())
    }

    pub(crate) fn push_budgeted_borrowed_row(
        &mut self,
        row: RowRef<'_>,
        account: &QueryMemoryAccount,
        retained: &mut QueryMemoryLease,
    ) -> Result<()> {
        let heap_bytes = row.iter().try_fold(0usize, |total, (_, value)| {
            checked_result_add(total, cloned_value_heap_bytes(value)?)
        })?;
        let next_count = self.prepare_budgeted_row(
            row.iter().map(|(name, _)| name),
            row.len(),
            heap_bytes,
            account,
            retained,
        )?;
        // Source buffers stay producer-owned. Only values that outlive this
        // call are copied, after their complete retained footprint is admitted.
        let schema = self.schema.as_ref().expect("result schema is bound");
        if row
            .iter()
            .map(|(name, _)| name)
            .eq(schema.columns().iter().map(String::as_str))
        {
            self.values
                .extend(row.iter().map(|(_, value)| value.to_owned_value()));
        } else {
            self.values.extend(schema.columns().iter().map(|name| {
                row.get(name)
                    .expect("row keys were validated before transfer")
                    .to_owned_value()
            }));
        }
        self.row_count = next_count;
        Ok(())
    }

    pub(crate) fn bind_budgeted_schema<'name>(
        &mut self,
        names: impl Iterator<Item = &'name str> + Clone,
        account: &QueryMemoryAccount,
        retained: &mut QueryMemoryLease,
    ) -> Result<()> {
        if self.schema.is_some() {
            return Ok(());
        }
        // Validate before allocating, including when execution produces no rows.
        for (index, name) in names.clone().enumerate() {
            if names.clone().take(index).any(|previous| previous == name) {
                return Err(HawDBError::Semantic(format!(
                    "query result schema contains duplicate column {name}"
                )));
            }
        }
        let slots = checked_result_mul(names.clone().count(), std::mem::size_of::<String>())?;
        let name_bytes = names
            .clone()
            .try_fold(0usize, |total, name| checked_result_add(total, name.len()))?;
        // Admit both the retained Arc and the overlapping conversion buffer.
        let staging = account.reserve(slots)?;
        retained.grow(checked_result_add(slots, name_bytes)?)?;
        self.schema = Some(QuerySchema {
            columns: Arc::from(names.map(str::to_owned).collect::<Vec<_>>()),
        });
        drop(staging);
        Ok(())
    }

    fn prepare_budgeted_row<'row>(
        &mut self,
        names: impl Iterator<Item = &'row str> + Clone,
        width: usize,
        heap_bytes: usize,
        account: &QueryMemoryAccount,
        retained: &mut QueryMemoryLease,
    ) -> Result<usize> {
        self.bind_budgeted_schema(names.clone(), account, retained)?;
        let schema = self.schema.as_ref().expect("result schema is bound");
        if width != schema.len()
            || (!names
                .clone()
                .eq(schema.columns().iter().map(String::as_str))
                && schema
                    .columns()
                    .iter()
                    .any(|name| names.clone().filter(|candidate| *candidate == name).count() != 1))
        {
            return Err(HawDBError::Execution(
                "query result row does not match its bound schema".into(),
            ));
        }
        let next_count = self
            .row_count
            .checked_add(1)
            .ok_or_else(result_capacity_overflow)?;
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
        Ok(next_count)
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

fn retained_map_node_bound(entries: usize) -> Result<usize> {
    // The pinned std B-tree has 11 slots, 12 child pointers, and at least
    // five entries per non-root node. Include an allocated empty root and
    // bound every node as internal. See OWNED_RESULT_MEMORY_ACCOUNTING.md;
    // toolchain upgrades must requalify this implementation-dependent bound.
    let nodes = checked_result_add(1, entries.saturating_sub(1) / 5)?;
    let slots = checked_result_mul(11, std::mem::size_of::<(String, Value)>())?;
    let pointers = checked_result_mul(14, std::mem::size_of::<usize>())?;
    checked_result_mul(nodes, checked_result_add(slots, pointers)?)
}

fn retained_value_heap_bytes(value: &Value) -> Result<usize> {
    match value {
        Value::String(value) => Ok(value.capacity()),
        Value::Binary(value) => Ok(value.capacity()),
        Value::Vector(value) => Ok(value.retained_capacity()),
        Value::List(values) => values.iter().try_fold(
            checked_result_mul(values.capacity(), std::mem::size_of::<Value>())?,
            |total, value| checked_result_add(total, retained_value_heap_bytes(value)?),
        ),
        Value::Map(values) => values.iter().try_fold(
            retained_map_node_bound(values.len())?,
            |total, (name, value)| {
                checked_result_add(
                    total,
                    checked_result_add(name.capacity(), retained_value_heap_bytes(value)?)?,
                )
            },
        ),
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Uuid(_) => Ok(0),
    }
}

/// Footprint of the owned copy, independent of spare capacity in its source.
/// The pinned Vec/String clone implementations allocate their logical length;
/// nested maps use the same conservative native-node bound as moved results.
fn cloned_value_heap_bytes(value: ValueRef<'_>) -> Result<usize> {
    match value {
        ValueRef::String(value) => Ok(value.len()),
        ValueRef::Binary(value) => Ok(value.len()),
        ValueRef::Vector(value) => Ok(value.encoded().len()),
        ValueRef::List(values) => values.iter().try_fold(
            checked_result_mul(values.len(), std::mem::size_of::<Value>())?,
            |total, value| checked_result_add(total, cloned_value_heap_bytes(value.as_ref())?),
        ),
        ValueRef::Map(values) => values.iter().try_fold(
            retained_map_node_bound(values.len())?,
            |total, (name, value)| {
                checked_result_add(
                    total,
                    checked_result_add(name.len(), cloned_value_heap_bytes(value.as_ref())?)?,
                )
            },
        ),
        ValueRef::Null
        | ValueRef::Bool(_)
        | ValueRef::Int(_)
        | ValueRef::Float(_)
        | ValueRef::Uuid(_) => Ok(0),
    }
}

pub(crate) fn cloned_row_memory_bytes(row: RowRef<'_>) -> Result<usize> {
    row.iter().try_fold(
        checked_result_add(
            std::mem::size_of::<Row>(),
            retained_map_node_bound(row.len())?,
        )?,
        |total, (name, value)| {
            checked_result_add(
                total,
                checked_result_add(name.len(), cloned_value_heap_bytes(value)?)?,
            )
        },
    )
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

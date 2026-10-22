// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

//! Prepay each expression occurrence, including repeated columns and sort keys.
//! Admission borrows input values; it must never evaluate or clone a payload.

use super::*;
use hawdb_sql::ExprKind;

pub(super) fn admit(
    select: &SelectStatement,
    row: &BoundRow<'_>,
    parameters: &[Value],
    account: &hawdb_executor::QueryMemoryAccount,
) -> Result<QueryMemoryLease> {
    let mut bytes = 1024usize;
    for projection in &select.projection {
        match projection {
            SelectProjection::Wildcard => {
                for binding in &row.bindings {
                    for (ordinal, column) in binding.schema.columns.iter().enumerate() {
                        bytes = bytes
                            .saturating_add(512)
                            .saturating_add(column.name.len())
                            .saturating_add(binding.value(ordinal)?.estimated_payload_bytes());
                    }
                }
            }
            SelectProjection::Expression { expression, alias } => {
                bytes = bytes
                    .saturating_add(alias.as_ref().map_or(0, String::len))
                    .saturating_add(expression_bytes(expression, row, parameters)?);
            }
        }
    }
    for item in &select.order_by {
        let payload = match resolve_relational_order_target(select, item)? {
            RelationalOrderTarget::InputColumn(column)
            | RelationalOrderTarget::ProjectionColumn { column, .. } => {
                resolve_column(row, column)?.estimated_payload_bytes()
            }
            RelationalOrderTarget::ProjectionExpression { expression, .. } => {
                expression_bytes(expression, row, parameters)?
            }
        };
        bytes = bytes.saturating_add(512).saturating_add(payload);
    }
    if let Some(predicate) = &select.selection {
        bytes = bytes.saturating_add(expression_bytes(predicate, row, parameters)?);
    }
    // Each scalar leaf may coexist with its evaluated relational value, output
    // value and sort-key copy. Map/slot/name overhead is included per occurrence.
    let scratch = select.selection.as_ref().map_or(Ok(0), |predicate| {
        predicate_scratch_bytes(predicate, row, parameters)
    })?;
    account.reserve(bytes.saturating_mul(4).saturating_add(scratch))
}

fn expression_bytes(
    expression: &SqlExpression,
    row: &BoundRow<'_>,
    parameters: &[Value],
) -> Result<usize> {
    let mut bytes = Ok(0usize);
    expression.visit(&mut |expression| {
        let Ok(total) = &mut bytes else {
            return;
        };
        let payload = match &expression.kind {
            ExprKind::Column(column) => resolve_column(row, column).map(|value| {
                value
                    .estimated_payload_bytes()
                    .saturating_add(column.name.len())
            }),
            ExprKind::Value(value) => {
                let value = match value {
                    SqlValue::Literal(value) => Some(value),
                    SqlValue::Parameter(position) => position
                        .checked_sub(1)
                        .and_then(|index| parameters.get(index)),
                };
                value
                    .map(hawdb_executor::binding::value_memory_bytes)
                    .ok_or_else(|| HawDBError::Semantic("missing SQL expression parameter".into()))
            }
            ExprKind::Function { name, .. } => Ok(name.len()),
            _ => Ok(0),
        };
        match payload {
            Ok(payload) => *total = total.saturating_add(512).saturating_add(payload),
            Err(error) => bytes = Err(error),
        }
    });
    bytes
}

fn predicate_scratch_bytes(
    predicate: &SqlExpression,
    row: &BoundRow<'_>,
    parameters: &[Value],
) -> Result<usize> {
    let mut bytes = Ok(0usize);
    predicate.visit(&mut |expression| {
        let Ok(total) = &mut bytes else {
            return;
        };
        if let ExprKind::Like {
            left,
            pattern,
            escape,
            case_insensitive,
            ..
        } = &expression.kind
        {
            let scratch = (|| {
                let (Some(value), Some(pattern)) = (
                    text_operand(left, row, parameters)?,
                    text_operand(pattern, row, parameters)?,
                ) else {
                    return Ok(0);
                };
                // An invalid layout also fails in the matcher before any buffer
                // allocation. Leave its diagnostic to actual predicate evaluation
                // so a short-circuited branch cannot introduce an earlier error.
                Ok(
                    hawdb_sql::sql_like_scratch_bytes(value, pattern, *escape, *case_insensitive)
                        .unwrap_or(0),
                )
            })();
            match scratch {
                Ok(scratch) => *total = total.saturating_add(scratch),
                Err(error) => bytes = Err(error),
            }
        }
    });
    bytes
}

// Borrow matcher inputs without invoking scalar evaluation or cloning a parameter.
// The predicate evaluator accepts a column on the left and Column/Value on the
// right. If it gains computed operands, this borrowing-only admission must grow
// with it. NULL has no workspace; unsupported operands retain its diagnostics.
fn text_operand<'a>(
    expression: &'a SqlExpression,
    row: &'a BoundRow<'_>,
    parameters: &'a [Value],
) -> Result<Option<&'a str>> {
    match &expression.kind {
        ExprKind::Column(column) => Ok(match resolve_column(row, column)? {
            RelationalValue::Text(value) => Some(value.as_str()),
            _ => None,
        }),
        ExprKind::Value(value) => {
            let value = match value {
                SqlValue::Literal(value) => Some(value),
                SqlValue::Parameter(position) => position
                    .checked_sub(1)
                    .and_then(|index| parameters.get(index)),
            };
            Ok(match value {
                Some(Value::String(value)) => Some(value.as_str()),
                _ => None,
            })
        }
        _ => Ok(None),
    }
}

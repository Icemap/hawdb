// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

pub(super) fn lower_source(table: &TableFactor) -> Result<Option<SqlVectorIndexSearch>> {
    let TableFactor::Table {
        name,
        args: Some(args),
        ..
    } = table
    else {
        return Ok(None);
    };
    let name = lower_table_name(name)?;
    if name.schema.is_some() || name.name != "vector_index_search" {
        return Err(HawDBError::Semantic(
            "unsupported SQL table function".into(),
        ));
    }
    if args.settings.is_some() {
        return Err(HawDBError::Semantic(
            "vector_index_search does not accept SETTINGS".into(),
        ));
    }
    let [FunctionArg::Unnamed(FunctionArgExpr::Expr(index)), FunctionArg::Unnamed(FunctionArgExpr::Expr(embedding)), options @ ..] =
        args.args.as_slice()
    else {
        return Err(HawDBError::Semantic(
            "vector_index_search requires index and embedding arguments".into(),
        ));
    };
    let mut candidates = None;
    let mut top_k = None;
    for option in options {
        let (name, value) = match option {
            FunctionArg::Named {
                name,
                arg: FunctionArgExpr::Expr(value),
                ..
            }
            | FunctionArg::ExprNamed {
                name: Expr::Identifier(name),
                arg: FunctionArgExpr::Expr(value),
                ..
            } => (name, value),
            _ => {
                return Err(HawDBError::Semantic(
                    "vector index options must be named".into(),
                ))
            }
        };
        let slot = match normalize_ident(name).as_str() {
            "candidates" => &mut candidates,
            "top_k" => &mut top_k,
            _ => return Err(HawDBError::Semantic("unknown vector index option".into())),
        };
        if slot.replace(lower_literal_expr(value)?).is_some() {
            return Err(HawDBError::Semantic("duplicate vector index option".into()));
        }
    }
    let required =
        || HawDBError::Semantic("vector index requires explicit candidates and top_k".into());
    Ok(Some(SqlVectorIndexSearch {
        index: lower_literal_expr(index)?,
        embedding: lower_literal_expr(embedding)?,
        candidates: candidates.ok_or_else(required)?,
        top_k: top_k.ok_or_else(required)?,
    }))
}

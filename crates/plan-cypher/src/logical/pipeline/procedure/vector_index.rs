// Copyright 2026 Nowledge
// Licensed under the Apache License, Version 2.0.

use super::*;

pub(super) fn bind_vector_index_pipeline(
    search: &hawdb_cypher::VectorIndexSearch,
    yields: &[YieldItem],
    tail: &[Clause],
    parameters: &BTreeMap<String, Value>,
) -> Result<LogicalPlan> {
    let Value::String(index) = bind_value(&search.index, parameters)? else {
        return Err(unsupported("vector index name must be a string"));
    };
    if index.is_empty() || index.len() > 128 {
        return Err(unsupported("vector index name must contain 1..128 bytes"));
    }
    let ValueExpressionKind::Parameter(parameter) = &search.embedding.kind else {
        return Err(unsupported(
            "vector index embedding must be a typed vector parameter",
        ));
    };
    if !matches!(parameters.get(parameter), Some(Value::Vector(_))) {
        return Err(unsupported(
            "vector index embedding must be a typed vector parameter",
        ));
    }
    let candidate_limit = bind_non_negative_usize(&search.candidates, parameters, "candidates")?;
    let top_k = bind_non_negative_usize(&search.top_k, parameters, "topK")?;
    if top_k == 0 || top_k > candidate_limit {
        return Err(unsupported("vector index requires 0 < topK <= candidates"));
    }
    let mut variable = "node".to_owned();
    let mut distance_column = "distance".to_owned();
    let mut scope = Scope::default();
    let mut seen = BTreeSet::new();
    for item in yields {
        let column = item.name.to_ascii_lowercase();
        if !seen.insert(column.clone()) {
            return Err(unsupported("vector index YIELD columns must be distinct"));
        }
        let alias = item.alias.as_ref().unwrap_or(&column);
        if scope.0.contains_key(alias) {
            return Err(unsupported("vector index YIELD aliases must be distinct"));
        }
        match column.as_str() {
            "node" => {
                variable = alias.clone();
                scope.bind_graph(alias, GraphEntityKind::Node, &mut Vec::new())?;
            }
            "distance" => {
                distance_column = alias.clone();
                scope.0.insert(alias.clone(), BindingType::Scalar);
            }
            _ => return Err(unsupported("unknown vector index YIELD column")),
        }
    }
    if yields.is_empty() {
        scope.bind_graph(&variable, GraphEntityKind::Node, &mut Vec::new())?;
        scope.0.insert(distance_column.clone(), BindingType::Scalar);
    }
    if variable == distance_column {
        return Err(unsupported(
            "vector index node and distance output names must be distinct",
        ));
    }
    let input = LogicalPlan::VectorIndexSearch(Box::new(crate::VectorIndexScan {
        index,
        embedding_parameter: parameter.clone(),
        candidate_limit,
        top_k,
        variable,
        distance_column,
        node_visibility_predicate: None,
    }));
    if tail.is_empty() {
        return Err(unsupported(
            "vector index search requires a RETURN projection",
        ));
    }
    bind_read_clauses(tail, Some(input), scope, parameters)
}

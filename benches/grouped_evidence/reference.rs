// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use hawdb::{Row, Value};
use std::collections::BTreeMap;

pub type FullGroups = BTreeMap<i64, BTreeMap<String, f64>>;

pub fn select_groups(groups: FullGroups, m: usize, k: usize) -> Vec<Row> {
    let mut ranked = groups
        .into_iter()
        .map(|(group, documents)| {
            let mut documents = documents.into_iter().collect::<Vec<_>>();
            documents.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            let best = documents[0].1;
            let evidence = documents
                .into_iter()
                .take(m)
                .map(|(id, _)| Value::String(id))
                .collect();
            (group, best, Value::List(evidence))
        })
        .collect::<Vec<_>>();
    ranked.sort_unstable_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    ranked
        .into_iter()
        .take(k)
        .map(|(g, best, evidence)| {
            BTreeMap::from([
                ("g".into(), Value::Int(g)),
                ("best".into(), Value::Float(best)),
                ("evidence".into(), evidence),
            ])
        })
        .collect()
}

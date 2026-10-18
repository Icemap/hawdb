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

use hawdb_core::{Catalog, Value};
use hawdb_storage::{mutation::GraphMutation, store::GraphStore};
use std::collections::BTreeMap;
use std::time::Instant;

#[test]
#[ignore = "manual release-mode pending merge scaling measurement"]
fn pending_merge_scaling_probe() {
    for rows in [8, 1_000, 10_000] {
        for distinct in [false, true] {
            let keys = if distinct { rows } else { rows.min(8) };
            for payload_bytes in [0, 4_096] {
                for sample in 0..4 {
                    let mut catalog = Catalog::default();
                    let mut store = GraphStore::default();
                    store
                        .create_property_index(&mut catalog, "Memory", "key")
                        .unwrap();
                    let payload = "x".repeat(payload_bytes);
                    let mutations = (0..rows)
                        .map(|index| GraphMutation::MergeNode {
                            label: "Memory".into(),
                            match_properties: BTreeMap::from([(
                                "key".into(),
                                Value::Int((index % keys) as i64),
                            )]),
                            on_create_properties: BTreeMap::from([(
                                "payload".into(),
                                Value::String(payload.clone()),
                            )]),
                            on_match_assignments: Vec::new(),
                            post_merge_assignments: Vec::new(),
                        })
                        .collect();
                    let start = Instant::now();
                    let result = store.commit_mutations(&mut catalog, mutations).unwrap();
                    let elapsed = start.elapsed();
                    assert_eq!(result.rows.len(), rows);
                    for (index, row) in result.rows.iter().enumerate() {
                        assert_eq!(row["node_id"], Value::Int((index % keys) as i64));
                        assert_eq!(row["created"], Value::Bool(index < keys));
                    }
                    assert_eq!(store.node_count_for_label(None), keys);
                    assert_eq!(store.relationship_records_owned().count(), 0);
                    for node in store.node_records_owned() {
                        let node = node.unwrap();
                        assert_eq!(node.properties["key"], Value::Int(node.id.0 as i64));
                        let Value::String(actual) = &node.properties["payload"] else {
                            panic!("payload must remain a string");
                        };
                        assert_eq!(actual.len(), payload_bytes);
                    }
                    println!(
                        "pending_merge rows={rows} distinct={distinct} payload_bytes={payload_bytes} sample={sample} elapsed_s={:.9}",
                        elapsed.as_secs_f64()
                    );
                }
            }
        }
    }
}

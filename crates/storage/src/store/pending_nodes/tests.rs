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

fn properties<const N: usize>(entries: [(&str, Value); N]) -> BTreeMap<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect()
}

fn assert_matches_scan(
    pending: &mut PendingNodes,
    label: LabelId,
    required: &BTreeMap<String, Value>,
) {
    for mode in [PendingMatch::Subset, PendingMatch::Exact] {
        let expected = pending
            .iter()
            .find(|(_, node_label, stored)| {
                *node_label == label
                    && match mode {
                        PendingMatch::Subset => required
                            .iter()
                            .all(|(key, value)| stored.get(key) == Some(value)),
                        PendingMatch::Exact => stored == required,
                    }
            })
            .map(|(id, _, _)| *id);
        assert_eq!(
            pending.find_by_properties(label, required, mode),
            expected,
            "label={label:?} required={required:?} mode={mode:?}"
        );
    }
}

#[test]
fn property_candidates_match_scan_for_all_values_labels_and_match_modes() {
    let values = [
        Value::Null,
        Value::Bool(false),
        Value::Bool(true),
        Value::Int(-1),
        Value::Int(0),
        Value::Float(-0.0),
        Value::Float(0.0),
        Value::Float(f64::NEG_INFINITY),
        Value::Float(f64::INFINITY),
        Value::Float(f64::from_bits(0x7ff8_0000_0000_0001)),
        Value::Float(f64::from_bits(0x7ff8_0000_0000_0002)),
        Value::String("0".into()),
        Value::Binary(vec![0, 255]),
        Value::Uuid("00000000-0000-0000-0000-000000000007".parse().unwrap()),
        Value::List(vec![Value::Null, Value::Int(1)]),
        Value::Map(properties([("nested", Value::Float(-0.0))])),
    ];
    let mut pending = PendingNodes::default();
    for (index, value) in values.iter().enumerate() {
        for offset in 0..3 {
            let mut stored = properties([("key", value.clone())]);
            if offset == 0 {
                stored.insert("payload".into(), Value::String("body".repeat(1024)));
            }
            pending.push((
                NodeId((index * 3 + offset) as u64),
                LabelId(u32::from(offset == 2)),
                stored,
            ));
        }
    }
    assert!(pending.property_indexes.is_none());
    for label in [LabelId(0), LabelId(1), LabelId(2)] {
        assert_matches_scan(&mut pending, label, &properties([]));
        for value in &values {
            assert_matches_scan(&mut pending, label, &properties([("key", value.clone())]));
            assert_matches_scan(
                &mut pending,
                label,
                &properties([("key", value.clone()), ("missing", Value::Null)]),
            );
        }
    }
    let indexes = pending.property_indexes.as_ref().unwrap();
    assert!(indexes
        .labels
        .values()
        .all(|properties| !properties.contains_key("payload")));
}

#[test]
fn lazy_indexes_track_assignments_deletions_new_labels_and_empty_postings() {
    let mut pending = PendingNodes::default();
    let key = |value| properties([("key", Value::Int(value))]);
    pending.push((NodeId(1), LabelId(0), key(10)));
    pending.push((NodeId(3), LabelId(0), key(10)));
    pending.set_property(1, "key".into(), Value::Int(20));
    pending.remove(NodeId(1));
    assert_matches_scan(&mut pending, LabelId(0), &properties([]));
    assert!(pending.property_indexes.is_none());
    // Exercise active index maintenance with a small, readable fixture.
    pending.property_indexes = Some(PropertyIndexes::default());
    assert_eq!(
        pending.find_by_properties(LabelId(0), &key(10), PendingMatch::Subset),
        None
    );
    pending.push((NodeId(7), LabelId(0), key(10)));
    assert_matches_scan(&mut pending, LabelId(0), &key(10));
    pending.set_property(0, "key".into(), Value::Int(10));
    pending.set_property(0, "other".into(), Value::Int(1));
    assert_eq!(
        pending.find_by_properties(LabelId(0), &key(10), PendingMatch::Subset),
        Some(NodeId(3))
    );
    assert_matches_scan(&mut pending, LabelId(0), &key(10));
    assert_matches_scan(
        &mut pending,
        LabelId(0),
        &properties([("key", Value::Int(10)), ("other", Value::Int(1))]),
    );
    pending.set_property(0, "key".into(), Value::Int(30));
    assert_matches_scan(&mut pending, LabelId(0), &key(10));
    pending.remove(NodeId(7));
    assert_matches_scan(&mut pending, LabelId(0), &key(10));
    pending.push((NodeId(9), LabelId(0), key(10)));
    assert_matches_scan(&mut pending, LabelId(0), &key(10));
    assert_matches_scan(&mut pending, LabelId(99), &key(10));
    pending.push((NodeId(11), LabelId(99), key(10)));
    assert_matches_scan(&mut pending, LabelId(99), &key(10));
    pending.remove(NodeId(123));
    assert_matches_scan(&mut pending, LabelId(99), &key(10));
}

#[test]
fn fingerprint_collisions_recheck_properties_and_keep_the_lowest_matching_id() {
    let mut pending = PendingNodes {
        property_indexes: Some(PropertyIndexes::default()),
        ..PendingNodes::default()
    };
    for (id, label, key, extra) in [
        (0, 1, 7, 0),
        (1, 0, 8, 0),
        (3, 0, 7, 1),
        (5, 0, 7, 0),
        (7, 0, 7, 0),
    ] {
        pending.push((
            NodeId(id),
            LabelId(label),
            properties([("key", Value::Int(key)), ("extra", Value::Int(extra))]),
        ));
    }
    let required = properties([("key", Value::Int(7))]);
    assert_matches_scan(&mut pending, LabelId(0), &required);
    let indexes = pending.property_indexes.as_mut().unwrap();
    let hash = indexes.hashes.hash_one(Value::Int(7));
    indexes
        .labels
        .get_mut(&LabelId(0))
        .unwrap()
        .get_mut("key")
        .unwrap()
        .get_mut(&hash)
        .unwrap()
        .extend([NodeId(0), NodeId(1), NodeId(2)]);
    assert_matches_scan(&mut pending, LabelId(0), &required);
    let residual = properties([("key", Value::Int(7)), ("extra", Value::Int(0))]);
    assert_eq!(
        pending.find_by_properties(LabelId(0), &residual, PendingMatch::Subset),
        Some(NodeId(5))
    );
    assert_matches_scan(&mut pending, LabelId(0), &residual);
    pending.remove(NodeId(3));
    pending.remove(NodeId(5));
    assert_matches_scan(&mut pending, LabelId(0), &required);
    assert_matches_scan(&mut pending, LabelId(0), &residual);
}

#[test]
fn equal_fingerprints_under_different_properties_are_independently_maintained() {
    let mut pending = PendingNodes {
        property_indexes: Some(PropertyIndexes::default()),
        ..PendingNodes::default()
    };
    pending.push((
        NodeId(1),
        LabelId(0),
        properties([("a", Value::Int(7)), ("b", Value::Int(7))]),
    ));
    for name in ["a", "b"] {
        assert_matches_scan(
            &mut pending,
            LabelId(0),
            &properties([(name, Value::Int(7))]),
        );
    }
    pending.set_property(0, "a".into(), Value::Int(8));
    assert_eq!(
        pending.find_by_properties(
            LabelId(0),
            &properties([("b", Value::Int(7))]),
            PendingMatch::Subset
        ),
        Some(NodeId(1))
    );
    assert_matches_scan(
        &mut pending,
        LabelId(0),
        &properties([("a", Value::Int(7))]),
    );
    assert_matches_scan(
        &mut pending,
        LabelId(0),
        &properties([("a", Value::Int(8)), ("b", Value::Int(7))]),
    );
}

#[test]
fn property_indexes_activate_only_after_the_small_batch_boundary() {
    let mut pending = PendingNodes::default();
    for id in 0..=PENDING_PROPERTY_SCAN_LIMIT {
        pending.push((
            NodeId(id as u64),
            LabelId(0),
            properties([("key", Value::Int(id as i64))]),
        ));
        if id == 5 {
            pending.set_property(id, "key".into(), Value::Int(-1));
        }
        assert_matches_scan(
            &mut pending,
            LabelId(0),
            &properties([("key", Value::Int(id as i64))]),
        );
        assert_eq!(
            pending.property_indexes.is_some(),
            id == PENDING_PROPERTY_SCAN_LIMIT
        );
    }
    assert_eq!(
        pending.find_by_properties(
            LabelId(0),
            &properties([("key", Value::Int(-1))]),
            PendingMatch::Subset
        ),
        Some(NodeId(5))
    );
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn canonical_decoding_refuses_instead_of_returning_only_the_live_overlay() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "hawdb-retained-source-{}-{nonce}",
        std::process::id()
    ));
    let config = WalReplayConfig {
        residency_mode: StorageResidencyMode::OutOfCore,
        ..WalReplayConfig::default()
    };
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &path,
        &mut catalog,
        DurabilityPolicy::default(),
        config,
    )
    .unwrap();
    store
        .create_node(
            &mut catalog,
            "Item",
            BTreeMap::from([("score".into(), Value::Int(1))]),
        )
        .unwrap();
    store.checkpoint(&catalog).unwrap();
    store
        .create_node(
            &mut catalog,
            "Item",
            BTreeMap::from([("score".into(), Value::Int(2))]),
        )
        .unwrap();
    assert!(store.is_out_of_core());
    let mut unexpected = |_| -> Result<Box<dyn GraphReadAllocation>> {
        panic!("unsupported point storage must not invoke admission");
    };
    let admission = GraphReadAdmission::new(&mut unexpected);
    assert!(matches!(
        store
            .materialized_node_point_read(NodeId(u64::MAX), &admission)
            .unwrap(),
        MaterializedNodePointRead::Unsupported
    ));

    assert!(store.try_materialized_node_read_source().unwrap().is_none());
    assert!(store
        .try_scan_materialized_nodes_after(catalog.label_id("Item"), None)
        .unwrap()
        .is_none());
    let mut count = 0;
    store
        .visit_nodes_owned(catalog.label_id("Item"), |_| {
            count += 1;
            GraphScanControl::Continue
        })
        .unwrap();
    assert_eq!(count, 2);
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn canonical_projected_point_admission_preserves_refusal_and_allows_retry() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "hawdb-projected-point-admission-{}-{nonce}",
        std::process::id()
    ));
    let config = WalReplayConfig {
        residency_mode: StorageResidencyMode::OutOfCore,
        ..WalReplayConfig::default()
    };
    let mut catalog = Catalog::default();
    let mut store = GraphStore::open_with_durability_and_replay_config(
        &path,
        &mut catalog,
        DurabilityPolicy::default(),
        config,
    )
    .unwrap();
    let id = store
        .create_node(
            &mut catalog,
            "Item",
            BTreeMap::from([
                ("score".into(), Value::Int(9)),
                ("content".into(), Value::String("x".repeat(128 * 1024))),
            ]),
        )
        .unwrap();
    store.checkpoint(&catalog).unwrap();
    drop(store);
    let store = GraphStore::open_with_durability_and_replay_config(
        &path,
        &mut catalog,
        DurabilityPolicy::default(),
        config,
    )
    .unwrap();
    assert!(store.is_out_of_core());
    assert!(!store.nodes.contains_key(&id));
    assert_eq!(
        store.storage_residency_report().segment_cache_pinned_bytes,
        0
    );
    let properties = BTreeSet::from(["score".into()]);
    let mut refused_bytes = None;
    let error = store
        .projected_node_owned_admitted(id, &properties, &mut |bytes| {
            assert!(refused_bytes.replace(bytes).is_none());
            assert!(store.storage_residency_report().segment_cache_pinned_bytes > 0);
            Err(HawDBError::Execution("admission sentinel".into()))
        })
        .unwrap_err();
    assert!(matches!(error, HawDBError::Execution(reason) if reason == "admission sentinel"));
    assert!(refused_bytes.unwrap() < 4096);
    assert_eq!(
        store.storage_residency_report().segment_cache_pinned_bytes,
        0
    );
    let mut retry_calls = 0;
    let node = store
        .projected_node_owned_admitted(id, &properties, &mut |bytes| {
            retry_calls += 1;
            assert_eq!(Some(bytes), refused_bytes);
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(retry_calls, 1);
    assert_eq!(
        store.storage_residency_report().segment_cache_pinned_bytes,
        0
    );
    assert_eq!(node.id, id);
    assert_eq!(
        node.properties,
        BTreeMap::from([("score".into(), Value::Int(9))])
    );
    assert!(store
        .projected_node_owned_admitted(NodeId(u64::MAX), &properties, &mut |_| {
            panic!("missing nodes must not invoke admission")
        })
        .unwrap()
        .is_none());
    drop(store);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn demanded_scan_resumes_borrowed_label_order_across_publication() {
    let mut store = GraphStore::default();
    let mut catalog = Catalog::default();
    for id in 0..1025 {
        store
            .create_node(
                &mut catalog,
                if id % 3 == 0 { "Other" } else { "Item" },
                BTreeMap::from([("score".into(), Value::Int(id))]),
            )
            .unwrap();
    }
    let label = catalog.label_id("Item").unwrap();
    let snapshot = store.snapshot_for_read();
    let expected = snapshot
        .scan_nodes(Some(label))
        .map(|node| node.id)
        .collect::<Vec<_>>();
    let mut last = None;
    let mut actual = Vec::new();
    loop {
        let mut batch_len = 0;
        for node in snapshot
            .try_scan_materialized_nodes_after(Some(label), last)
            .unwrap()
            .unwrap()
            .take(17)
        {
            assert!(std::ptr::eq(node, snapshot.nodes.get(&node.id).unwrap()));
            last = Some(node.id);
            actual.push(node.id);
            batch_len += 1;
        }
        if batch_len == 0 {
            break;
        }
        store
            .create_node(
                &mut catalog,
                "Item",
                BTreeMap::from([("score".into(), Value::Int(-1))]),
            )
            .unwrap();
    }
    assert_eq!(actual, expected);
    assert!(store.scan_nodes(Some(label)).count() > expected.len());
    assert!(snapshot
        .try_scan_materialized_nodes_after(None, Some(NodeId(u64::MAX)))
        .unwrap()
        .unwrap()
        .next()
        .is_none());
}

#[test]
fn minimal_source_shares_records_and_survives_store_mutation_and_destruction() {
    let mut store = GraphStore::default();
    let mut catalog = Catalog::default();
    for score in 0..4 {
        store
            .create_node(
                &mut catalog,
                "Item",
                BTreeMap::from([("score".into(), Value::Int(score))]),
            )
            .unwrap();
    }
    let source = store.try_materialized_node_read_source().unwrap().unwrap();
    assert_eq!(source.row_count(), 4);
    assert_eq!(source.page_count(), store.nodes.segment_count());
    assert_eq!(
        source.directory_capacity_bytes(),
        store.nodes.directory_capacity_bytes()
    );
    assert!(source.nodes.shares_storage_with(&store.nodes));
    let original = source.iter_after(None).unwrap().next().unwrap();
    assert!(std::ptr::eq(
        original,
        store.nodes.get(&original.id).unwrap()
    ));
    let first_id = original.id;
    store
        .nodes
        .get_mut(&first_id)
        .unwrap()
        .properties
        .insert("score".into(), Value::Int(99));
    assert_eq!(original.properties["score"], Value::Int(0));
    assert_eq!(
        store.nodes.get(&first_id).unwrap().properties["score"],
        Value::Int(99)
    );
    drop(store);
    let scores = source
        .iter_after(Some(first_id))
        .unwrap()
        .map(|node| node.properties["score"].clone())
        .collect::<Vec<_>>();
    assert_eq!(scores, vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
}

#[test]
fn minimal_source_observes_both_shared_poison_signals() {
    for integrity in [false, true] {
        let store = GraphStore::default();
        let source = store.try_materialized_node_read_source().unwrap().unwrap();
        assert!(source.iter_after(None).unwrap().next().is_none());
        if integrity {
            store
                .integrity_poisoned
                .store(true, AtomicOrdering::Release);
        } else {
            store
                .post_wal_apply_poisoned
                .store(true, AtomicOrdering::Release);
        }
        let Err(error) = source.iter_after(None) else {
            panic!("poisoned source must fail before yielding records");
        };
        assert_eq!(
            error.to_string(),
            store.ensure_usable().unwrap_err().to_string()
        );
    }
}

struct PointReadAllocation {
    bytes: usize,
    retained: std::rc::Rc<std::cell::Cell<bool>>,
    released: std::rc::Rc<std::cell::Cell<bool>>,
}

impl GraphReadAllocation for PointReadAllocation {
    fn bytes(&self) -> usize {
        self.bytes
    }

    fn grow(&mut self, bytes: usize) -> Result<()> {
        self.bytes += bytes;
        Ok(())
    }

    fn retain_state(&mut self) -> Result<()> {
        self.retained.set(true);
        Ok(())
    }
}

impl Drop for PointReadAllocation {
    fn drop(&mut self) {
        self.released.set(true);
    }
}

#[test]
fn admitted_point_source_preserves_addresses_and_reservation_until_release() {
    let mut store = GraphStore::default();
    let mut catalog = Catalog::default();
    let mut text = String::with_capacity(8192);
    text.push_str("original");
    let id = store
        .create_node(
            &mut catalog,
            "Item",
            BTreeMap::from([("text".into(), Value::String(text))]),
        )
        .unwrap();
    let node_address = std::ptr::from_ref(store.nodes.get(&id).unwrap());
    let retained = std::rc::Rc::new(std::cell::Cell::new(false));
    let released = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut allocate = |bytes| -> Result<Box<dyn GraphReadAllocation>> {
        assert!(bytes >= 8192);
        Ok(Box::new(PointReadAllocation {
            bytes,
            retained: retained.clone(),
            released: released.clone(),
        }))
    };
    let admission = GraphReadAdmission::new(&mut allocate);
    let MaterializedNodePointRead::Node(source) =
        store.materialized_node_point_read(id, &admission).unwrap()
    else {
        panic!("resident node must have an admitted source");
    };
    assert!(retained.get());
    assert!(!released.get());
    assert_eq!(std::ptr::from_ref(source.node().unwrap()), node_address);
    let Value::String(original) = &source.node().unwrap().properties["text"] else {
        panic!("fixture must retain a string");
    };
    let text_address = original.as_ptr();
    store.nodes.get_mut(&id).unwrap().properties.clear();
    drop(store);
    let Value::String(text) = &source.node().unwrap().properties["text"] else {
        panic!("source must survive store mutation and destruction");
    };
    assert_eq!(text, "original");
    assert_eq!(text.as_ptr(), text_address);
    assert!(!released.get());
    drop(source);
    assert!(released.get());
}

#[test]
fn point_source_rejects_short_permits_before_pin_and_missing_skips_admission() {
    let mut store = GraphStore::default();
    let mut catalog = Catalog::default();
    let id = store
        .create_node(&mut catalog, "Item", BTreeMap::new())
        .unwrap();
    let (page, _) = store.nodes.retained_point_page_preflight(id).unwrap();
    let owners = Arc::strong_count(page);
    let mut short =
        |_| -> Result<Box<dyn GraphReadAllocation>> { Ok(Box::new(UnaccountedReadAllocation(0))) };
    let admission = GraphReadAdmission::new(&mut short);
    assert!(matches!(
        store.materialized_node_point_read(id, &admission),
        Err(HawDBError::Execution(reason)) if reason.contains("insufficient allocation permit")
    ));
    assert_eq!(Arc::strong_count(page), owners);
    let mut refuse = |_| -> Result<Box<dyn GraphReadAllocation>> {
        Err(HawDBError::Execution("point admission sentinel".into()))
    };
    let admission = GraphReadAdmission::new(&mut refuse);
    assert!(matches!(
        store.materialized_node_point_read(id, &admission),
        Err(HawDBError::Execution(reason)) if reason == "point admission sentinel"
    ));
    assert_eq!(Arc::strong_count(page), owners);
    assert!(matches!(
        store
            .materialized_node_point_read(NodeId(u64::MAX), &admission)
            .unwrap(),
        MaterializedNodePointRead::Missing
    ));
    store.ensure_usable().unwrap();
}

#[test]
fn admitted_point_source_observes_later_poison() {
    let mut store = GraphStore::default();
    let mut catalog = Catalog::default();
    let id = store
        .create_node(&mut catalog, "Item", BTreeMap::new())
        .unwrap();
    let mut allocate = |bytes| -> Result<Box<dyn GraphReadAllocation>> {
        Ok(Box::new(UnaccountedReadAllocation(bytes)))
    };
    let admission = GraphReadAdmission::new(&mut allocate);
    let MaterializedNodePointRead::Node(source) =
        store.materialized_node_point_read(id, &admission).unwrap()
    else {
        panic!("resident point must be supported");
    };
    store
        .post_wal_apply_poisoned
        .store(true, AtomicOrdering::Release);
    assert!(source.node().is_err());
}

// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// Page-local metadata travels with the immutable page, rather than every
/// record owner. Arbitrary mutable access invalidates the cached estimate.
#[derive(Debug)]
pub(super) struct CowMapPage<K, V> {
    pub(super) entries: BTreeMap<K, V>,
    pub(super) bytes: OnceLock<u128>,
    pub(super) epoch_bounds: OnceLock<(u64, u64)>,
    // Destroy entries before refunding the physical page's history credit.
    pub(super) history_lease: OnceLock<Arc<crate::version::history_budget::HistoryLease>>,
}

impl<K: Clone, V: Clone> Clone for CowMapPage<K, V> {
    fn clone(&self) -> Self {
        Self {
            entries: self.entries.clone(),
            bytes: self.bytes.clone(),
            epoch_bounds: self.epoch_bounds.clone(),
            history_lease: OnceLock::new(),
        }
    }
}

impl<K, V> From<BTreeMap<K, V>> for CowMapPage<K, V> {
    fn from(entries: BTreeMap<K, V>) -> Self {
        Self {
            entries,
            bytes: OnceLock::new(),
            epoch_bounds: OnceLock::new(),
            history_lease: OnceLock::new(),
        }
    }
}

impl<K, V> Deref for CowMapPage<K, V> {
    type Target = BTreeMap<K, V>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl<K, V> DerefMut for CowMapPage<K, V> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.bytes.take();
        self.epoch_bounds.take();
        &mut self.entries
    }
}

impl<K: Ord + CowPageWeight, V: CowPageWeight> CowMapPage<K, V> {
    pub(super) fn payload_bytes(&self) -> u128 {
        *self.bytes.get_or_init(|| {
            self.entries
                .iter()
                .map(|(key, value)| key.cow_page_bytes() as u128 + value.cow_page_bytes() as u128)
                .sum()
        })
    }

    pub(super) fn estimated_bytes(&self) -> u128 {
        self.payload_bytes()
            + self.entries.len() as u128 * (4 * std::mem::size_of::<usize>()) as u128
    }

    pub(super) fn insert(&mut self, key: K, value: V) -> Option<V> {
        let bytes = self.payload_bytes();
        // BTreeMap retains the old key when Ord compares equal. Its weight
        // need not match the incoming key's weight.
        let (before, after) = self.entries.get_key_value(&key).map_or_else(
            || {
                (
                    0,
                    key.cow_page_bytes() as u128 + value.cow_page_bytes() as u128,
                )
            },
            |(stored_key, old)| {
                (
                    stored_key.cow_page_bytes() as u128 + old.cow_page_bytes() as u128,
                    stored_key.cow_page_bytes() as u128 + value.cow_page_bytes() as u128,
                )
            },
        );
        self.epoch_bounds.take();
        let previous = self.entries.insert(key, value);
        self.bytes = OnceLock::from(bytes - before + after);
        previous
    }

    pub(super) fn remove<Q: Ord + ?Sized>(&mut self, key: &Q) -> Option<V>
    where
        K: Borrow<Q>,
    {
        let bytes = self.payload_bytes();
        let removed_bytes = self.entries.get_key_value(key).map_or(0, |(key, value)| {
            key.cow_page_bytes() as u128 + value.cow_page_bytes() as u128
        });
        self.epoch_bounds.take();
        let removed = self.entries.remove(key);
        self.bytes = OnceLock::from(bytes - removed_bytes);
        removed
    }

    pub(super) fn split_off(&mut self, key: &K) -> Self {
        let bytes = self.payload_bytes();
        self.epoch_bounds.take();
        let right = Self::from(self.entries.split_off(key));
        self.bytes = OnceLock::from(bytes - right.payload_bytes());
        right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::rc::Rc;

    #[derive(Clone)]
    struct Weight(usize, Rc<Cell<usize>>);
    impl CowPageWeight for Weight {
        fn cow_page_bytes(&self) -> usize {
            self.1.set(self.1.get() + 1);
            self.0
        }
    }

    fn assert_weights(map: &CowSegmentedMap<u64, Weight>) {
        for page in map.segments.iter() {
            let expected: u128 = page
                .iter()
                .map(|(key, value)| {
                    key.cow_page_bytes() as u128
                        + value.cow_page_bytes() as u128
                        + (4 * std::mem::size_of::<usize>()) as u128
                })
                .sum();
            assert_eq!(page.estimated_bytes(), expected);
        }
    }

    #[test]
    fn replacements_reuse_cached_weight_and_preserve_snapshot_metadata() {
        let calls = Rc::new(Cell::new(0));
        let mut map = CowSegmentedMap::from(
            (0..256)
                .map(|id| (id, Weight(16, calls.clone())))
                .collect::<BTreeMap<_, _>>(),
        );
        assert_weights(&map);
        let snapshot = map.clone();
        calls.set(0);
        map.insert(0, Weight(32, calls.clone()));
        assert!(calls.get() < 10, "replacement weighed untouched values");
        assert_eq!(snapshot.get(&0).unwrap().0, 16);
        assert_weights(&snapshot);
        assert_weights(&map);
    }

    #[test]
    fn mutable_access_split_remove_and_panicking_retain_keep_exact_weights() {
        let calls = Rc::new(Cell::new(0));
        let mut map = CowSegmentedMap::from(
            (0..8)
                .map(|id| (id, Weight(16, calls.clone())))
                .collect::<BTreeMap<_, _>>(),
        );
        map.get_mut(&0).unwrap().0 = COW_MAP_TARGET_SEGMENT_BYTES;
        map.rebalance_key(&0);
        assert!(map.segment_count() > 1);
        assert_weights(&map);
        map.remove(&0);
        assert_weights(&map);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            map.retain(|_, value| {
                value.0 = 64;
                panic!("stop retain")
            });
        }));
        assert!(result.is_err());
        assert_weights(&map);
        map.get_mut(&1).unwrap().0 = usize::MAX;
        assert_weights(&map);
        map.get_mut(&1).unwrap().0 = 1;
        assert_weights(&map);
        map.retain(|key, value| {
            value.0 = 4;
            key % 2 == 0
        });
        assert_weights(&map);
    }
}

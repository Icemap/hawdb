// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::{
    relational_key_has_prefix, RelationalIndexRangeScan, RelationalIndexScanDirection,
    RelationalKey,
};
use std::cmp::Ordering;

/// A boundary on a leading index-key prefix. Inclusiveness applies to every
/// complete key sharing that prefix, including all trailing-column values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationalIndexBound {
    pub key: RelationalKey,
    pub inclusive: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelationalIndexBounds {
    pub lower: Option<RelationalIndexBound>,
    pub upper: Option<RelationalIndexBound>,
    /// A bound to SQL NULL, or another proven empty conjunction.
    pub empty: bool,
}

fn prefix_cmp(key: &RelationalKey, bound: &RelationalKey) -> Ordering {
    key.0[..key.0.len().min(bound.0.len())].cmp(&bound.0)
}

static UNBOUNDED: RelationalIndexBounds = RelationalIndexBounds {
    lower: None,
    upper: None,
    empty: false,
};

impl RelationalIndexRangeScan {
    pub(crate) fn bounds(&self) -> &RelationalIndexBounds {
        self.bounds.as_deref().unwrap_or(&UNBOUNDED)
    }

    pub(crate) fn lower_seek(&self) -> &RelationalKey {
        let mut key = &self.prefix;
        if let Some(lower) = &self.bounds().lower
            && &lower.key > key
        {
            key = &lower.key;
        }
        if self.direction == RelationalIndexScanDirection::Forward
            && let Some(cursor) = &self.exclusive_bound
            && cursor > key
        {
            key = cursor;
        }
        key
    }

    pub(crate) fn before(&self, key: &RelationalKey) -> bool {
        key < &self.prefix
            || self.bounds().lower.as_ref().is_some_and(|bound| {
                let order = prefix_cmp(key, &bound.key);
                order == Ordering::Less || (order == Ordering::Equal && !bound.inclusive)
            })
            || (self.direction == RelationalIndexScanDirection::Forward
                && self
                    .exclusive_bound
                    .as_ref()
                    .is_some_and(|bound| key <= bound))
    }

    pub(crate) fn after(&self, key: &RelationalKey) -> bool {
        (key >= &self.prefix && !relational_key_has_prefix(key, &self.prefix))
            || self.bounds().upper.as_ref().is_some_and(|bound| {
                let order = prefix_cmp(key, &bound.key);
                order == Ordering::Greater || (order == Ordering::Equal && !bound.inclusive)
            })
            || (self.direction == RelationalIndexScanDirection::Backward
                && self
                    .exclusive_bound
                    .as_ref()
                    .is_some_and(|bound| key >= bound))
    }

    pub fn matches(&self, key: &RelationalKey) -> bool {
        !self.bounds().empty && !self.before(key) && !self.after(key)
    }

    pub(crate) fn encoded_bounds<E>(
        &self,
        mut encode: impl FnMut(&RelationalKey) -> Result<Vec<u8>, E>,
    ) -> Result<EncodedRelationalIndexRange, E> {
        // Preserve rejection of an unbounded empty-prefix lookup. Only a
        // genuinely constrained first-column interval authorizes this shape.
        let constrained =
            self.bounds().lower.is_some() || self.bounds().upper.is_some() || self.bounds().empty;
        let prefix = if self.prefix.0.is_empty() && constrained {
            Vec::new()
        } else {
            encode(&self.prefix)?
        };
        let mut range = EncodedRelationalIndexRange {
            upper: successor(&prefix),
            lower: prefix,
            lower_inclusive: true,
            empty: self.bounds().empty,
        };
        if let Some(bound) = &self.bounds().lower {
            let key = encode(&bound.key)?;
            if bound.inclusive {
                range.include_lower(key, true);
            } else if let Some(next) = successor(&key) {
                range.include_lower(next, true);
            } else {
                range.empty = true;
            }
        }
        if let Some(bound) = &self.bounds().upper {
            let key = encode(&bound.key)?;
            let upper = if bound.inclusive {
                successor(&key)
            } else {
                Some(key)
            };
            if let Some(upper) = upper {
                range.include_upper(upper);
            }
        }
        if let Some(cursor) = &self.exclusive_bound {
            let key = encode(cursor)?;
            match self.direction {
                RelationalIndexScanDirection::Forward => range.include_lower(key, false),
                RelationalIndexScanDirection::Backward => range.include_upper(key),
            }
        }
        range.empty |= range
            .upper
            .as_ref()
            .is_some_and(|upper| &range.lower >= upper);
        Ok(range)
    }
}

pub(crate) struct EncodedRelationalIndexRange {
    pub(crate) lower: Vec<u8>,
    pub(crate) lower_inclusive: bool,
    pub(crate) upper: Option<Vec<u8>>,
    pub(crate) empty: bool,
}

impl EncodedRelationalIndexRange {
    fn include_lower(&mut self, key: Vec<u8>, inclusive: bool) {
        match key.cmp(&self.lower) {
            Ordering::Greater => {
                self.lower = key;
                self.lower_inclusive = inclusive;
            }
            Ordering::Equal => self.lower_inclusive &= inclusive,
            Ordering::Less => {}
        }
    }

    fn include_upper(&mut self, key: Vec<u8>) {
        if self.upper.as_ref().is_none_or(|upper| &key < upper) {
            self.upper = Some(key);
        }
    }

    pub(crate) fn before(&self, key: &[u8]) -> bool {
        key < self.lower.as_slice() || (!self.lower_inclusive && key == self.lower)
    }

    pub(crate) fn after(&self, key: &[u8]) -> bool {
        self.upper
            .as_ref()
            .is_some_and(|upper| key >= upper.as_slice())
    }

    pub(crate) fn contains(&self, key: &[u8]) -> bool {
        !self.empty && !self.before(key) && !self.after(key)
    }

    pub(crate) fn overlaps(&self, lower: &[u8], upper: &[u8]) -> bool {
        !self.empty && !self.before(upper) && !self.after(lower)
    }
}

fn successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut next = prefix.to_vec();
    while let Some(last) = next.pop() {
        if let Some(last) = last.checked_add(1) {
            next.push(last);
            return Some(next);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relational::{encode_relational_primary_key, RelationalValue};

    #[test]
    fn encoded_partial_bounds_agree_with_scalar_intersections_and_full_cursors() {
        for values in [
            vec![i64::MIN, -1, 0, 1, i64::MAX]
                .into_iter()
                .map(RelationalValue::BigInt)
                .collect::<Vec<_>>(),
            ["", "a", "a\0", "aa", "z"]
                .into_iter()
                .map(|s| RelationalValue::Text(s.into()))
                .collect(),
            vec![vec![], vec![0], vec![0, 255], vec![255], vec![255, 255]]
                .into_iter()
                .map(RelationalValue::Bytea)
                .collect(),
        ] {
            for prefixed in [false, true] {
                let prefix = if prefixed {
                    vec![RelationalValue::Text("tenant".into())]
                } else {
                    vec![]
                };
                let make_key = |value: &RelationalValue, suffix: Option<i64>| {
                    let mut key = prefix.clone();
                    key.push(value.clone());
                    if let Some(suffix) = suffix {
                        key.push(RelationalValue::BigInt(suffix));
                    }
                    RelationalKey(key)
                };
                for lower in 0..values.len() {
                    for upper in 0..values.len() {
                        for lower_inclusive in [false, true] {
                            for upper_inclusive in [false, true] {
                                for direction in [
                                    RelationalIndexScanDirection::Forward,
                                    RelationalIndexScanDirection::Backward,
                                ] {
                                    for cursor in [None, Some(make_key(&values[2], Some(1)))] {
                                        let scan = RelationalIndexRangeScan {
                                            prefix: RelationalKey(prefix.clone()),
                                            bounds: Some(Box::new(RelationalIndexBounds {
                                                lower: Some(RelationalIndexBound {
                                                    key: make_key(&values[lower], None),
                                                    inclusive: lower_inclusive,
                                                }),
                                                upper: Some(RelationalIndexBound {
                                                    key: make_key(&values[upper], None),
                                                    inclusive: upper_inclusive,
                                                }),
                                                empty: false,
                                            })),
                                            exclusive_bound: cursor.clone(),
                                            direction,
                                        };
                                        let encoded = scan
                                            .encoded_bounds(encode_relational_primary_key)
                                            .unwrap();
                                        for value in &values {
                                            for suffix in [0, 1, 2] {
                                                let key = make_key(value, Some(suffix));
                                                let expected = (value > &values[lower] || (lower_inclusive && value == &values[lower]))
                                                    && (value < &values[upper] || (upper_inclusive && value == &values[upper]))
                                                    && cursor.as_ref().is_none_or(|cursor| match direction {
                                                        RelationalIndexScanDirection::Forward => &key > cursor,
                                                        RelationalIndexScanDirection::Backward => &key < cursor,
                                                    });
                                                assert_eq!(
                                                    scan.matches(&key),
                                                    expected,
                                                    "{scan:?} {key:?}"
                                                );
                                                assert_eq!(
                                                    encoded.contains(
                                                        &encode_relational_primary_key(&key)
                                                            .unwrap()
                                                    ),
                                                    expected,
                                                    "{scan:?} {key:?}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

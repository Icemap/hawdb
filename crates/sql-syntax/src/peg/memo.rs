// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use super::Memo;

pub(super) struct MemoTable {
    // Most statement-envelope rules are visited at only one token position.
    // Keep that first entry indexed by the compiled rule ID, without hashing.
    first: Vec<Option<(usize, Memo)>>,
    // Other positions retain ordinary packrat behavior, including failures.
    overflow: HashMap<(usize, usize), Memo>,
    len: usize,
}

impl MemoTable {
    pub fn new(rules: usize) -> Self {
        Self {
            first: std::iter::repeat_with(|| None).take(rules).collect(),
            overflow: HashMap::new(),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, rule: usize, position: usize) -> Option<&Memo> {
        match self.first[rule].as_ref() {
            Some((first_position, memo)) if *first_position == position => Some(memo),
            None => None,
            Some(_) => self.overflow.get(&(rule, position)),
        }
    }

    pub fn insert(&mut self, rule: usize, position: usize, memo: Memo) {
        match &mut self.first[rule] {
            slot @ None => {
                *slot = Some((position, memo));
                self.len += 1;
            }
            Some((first_position, first_memo)) if *first_position == position => {
                *first_memo = memo;
            }
            Some(_) => {
                if self.overflow.insert((rule, position), memo).is_none() {
                    self.len += 1;
                }
            }
        }
    }
}

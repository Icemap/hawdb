// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::version::{VersionKey, VersionStamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionPageRetention {
    Keep,
    Drop,
    Mixed,
}

impl CowSegmentedMap<VersionKey, VersionStamp> {
    /// Identity, payload charge and whether editing this page would detach it.
    /// The immutable page caches payload weights; this visits pages, not keys.
    pub(crate) fn version_page_layout(
        &self,
    ) -> impl Iterator<Item = (usize, usize, bool, bool)> + '_ {
        self.segments.iter().map(|page| {
            (
                Arc::as_ptr(&page.0) as usize,
                usize::try_from(page.payload_bytes()).expect("admitted version page weight"),
                self.segments.is_shared() || page.is_shared(),
                page.history_lease.get().is_some(),
            )
        })
    }

    pub(crate) fn charge_version_pages(
        &self,
        reservation: &mut Option<Arc<crate::version::history_budget::HistoryLease>>,
    ) {
        for page in self.segments.iter() {
            if page.history_lease.get().is_none() {
                let bytes = usize::try_from(page.payload_bytes()).expect("admitted page weight");
                let credit =
                    Arc::get_mut(reservation.as_mut().expect("new page has preflight credit"))
                        .expect("unshared preflight credit")
                        .split(bytes);
                // Concurrent captures can adopt the same unmanaged source page.
                // A losing credit refunds itself; physical ownership stays unique.
                let _ = page.history_lease.set(credit);
            }
        }
    }

    pub(crate) fn version_page_for_key(&self, key: &VersionKey) -> Option<usize> {
        self.segment_index(key)
            .map(|index| Arc::as_ptr(&self.segments[index].0) as usize)
    }

    pub(crate) fn version_retention_plan(
        &self,
        keep: &impl Fn(&VersionStamp) -> bool,
        floor: Option<u64>,
    ) -> Vec<VersionPageRetention> {
        self.segments
            .iter()
            .map(|page| {
                if let Some(floor) = floor {
                    let (min, max) = *page.epoch_bounds.get_or_init(|| {
                        page.values().fold((u64::MAX, 0), |(min, max), stamp| {
                            (min.min(stamp.commit_epoch), max.max(stamp.commit_epoch))
                        })
                    });
                    if min >= floor {
                        return VersionPageRetention::Keep;
                    }
                    if max < floor {
                        return VersionPageRetention::Drop;
                    }
                    return VersionPageRetention::Mixed;
                }
                if page.values().all(keep) {
                    VersionPageRetention::Keep
                } else if page.values().all(|stamp| !keep(stamp)) {
                    VersionPageRetention::Drop
                } else {
                    VersionPageRetention::Mixed
                }
            })
            .collect()
    }

    pub(crate) fn retain_version_pages(
        &mut self,
        plan: &[VersionPageRetention],
        keep: &impl Fn(&VersionStamp) -> bool,
    ) {
        debug_assert_eq!(self.delta_pressure_bytes, 0);
        let pages = &mut *self.segments;
        let mut index = 0;
        pages.retain_mut(|page| {
            let action = plan[index];
            index += 1;
            match action {
                VersionPageRetention::Keep => true,
                VersionPageRetention::Drop => false,
                VersionPageRetention::Mixed => {
                    let page = &mut **page;
                    page.retain(|_, stamp| keep(stamp));
                    let bytes =
                        usize::try_from(page.payload_bytes()).expect("admitted page weight");
                    if let Some(credit) = page.history_lease.get_mut() {
                        Arc::get_mut(credit)
                            .expect("unique page owns its credit")
                            .shrink(bytes);
                    }
                    !page.is_empty()
                }
            }
        });
        self.len = pages.iter().map(|page| page.len()).sum();
    }
}

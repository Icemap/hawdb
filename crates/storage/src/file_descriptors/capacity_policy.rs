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

use super::{file_descriptor_error, FileDescriptorError, Fixture, ProjectFileDescriptors};
use crate::file_descriptors::{DescriptorCache, DescriptorKind};
use std::sync::Arc;

#[cfg(any(unix, windows))]
#[test]
fn namespace_probes_isolate_exhausted_projects_and_preserve_alias_accounting() {
    const NAME: &str = "file_descriptors::tests::capacity_policy::namespace_probes_isolate_exhausted_projects_and_preserve_alias_accounting";
    if super::run_descriptor_child(NAME) {
        return;
    }
    let first = Fixture::new(1);
    let second = Fixture::new(1);
    let held = first
        .project
        .io_context()
        .acquire(DescriptorKind::Transient)
        .unwrap();
    let first_parent = first.root.join("probe-parent");
    let second_parent = second.root.join("probe-parent");
    std::fs::create_dir(&first_parent).unwrap();
    std::fs::create_dir(&second_parent).unwrap();
    let first_alias = first_parent.join("..");
    let second_alias = second_parent.join("..");
    let before = first.project.metrics();
    let native_before = super::native_descriptor_count();
    for _ in 0..16 {
        let independent = ProjectFileDescriptors::acquire_existing(&second_alias, 1).unwrap();
        assert!(Arc::ptr_eq(&independent.state, &second.project.state));
        let same = ProjectFileDescriptors::acquire_existing(&first_alias, 1).unwrap();
        assert!(Arc::ptr_eq(&same.state, &first.project.state));
        assert!(same
            .io_context()
            .acquire(DescriptorKind::Transient)
            .is_err());
        assert!(matches!(
            ProjectFileDescriptors::acquire_existing(&first_alias, 2),
            Err(super::HawDBError::FileDescriptors(
                FileDescriptorError::ConfigurationConflict { .. }
            ))
        ));
        assert!(ProjectFileDescriptors::acquire_existing(&second.root.join("missing"), 1).is_err());
        assert_eq!(first.project.metrics().open, before.open);
        assert_eq!(first.project.metrics().reserved, before.reserved);
        assert_eq!(first.project.metrics().high_water, before.high_water);
    }
    assert_eq!(super::native_descriptor_count(), native_before);
    let probes = super::super::NAMESPACE_PROBES.metrics();
    assert_eq!(probes.open, 0);
    assert_eq!(probes.reserved, 0);
    assert_eq!(probes.high_water, 1);
    drop(held);
}

#[test]
fn soft_capacity_preserves_pinned_readers_and_reclaims_idle_overshoot() {
    let fixture = Fixture::with_capacity_limit(2, false);
    let context = fixture.project.io_context();
    let bindings = (0..4)
        .map(|id| fixture.binding(&format!("reader-{id}"), &[id]))
        .collect::<Vec<_>>();
    let read = |index: usize| {
        fixture
            .project
            .immutable_handles
            .get(&bindings[index], &context)
            .unwrap()
    };
    let first = read(0);
    let second = read(1);
    let third = read(2);
    assert_eq!(fixture.project.metrics().cached_handles, 3);
    assert_eq!(fixture.project.metrics().limit, 2);
    assert_eq!(fixture.project.metrics().budget_rejections, 0);
    let mut byte = [255];
    crate::io::read_exact_at(&first, &mut byte, 0).unwrap();
    assert_eq!(byte, [0]);
    drop((second, third));
    let fourth = read(3);
    assert_eq!(fixture.project.metrics().cached_handles, 2);
    assert_eq!(fixture.project.metrics().cache_evictions, 2);
    crate::io::read_exact_at(&first, &mut byte, 0).unwrap();
    assert_eq!(byte, [0]);
    drop((first, fourth));
    fixture.project.immutable_handles.evict_idle(usize::MAX);
    assert_eq!(fixture.project.metrics().open, 0);
    assert_eq!(fixture.project.metrics().reserved, 0);
}

#[test]
fn strict_capacity_refuses_new_readers_without_reclaiming_a_pin() {
    let fixture = Fixture::new(1);
    let context = fixture.project.io_context();
    let first_binding = fixture.binding("first", b"first");
    let second_binding = fixture.binding("second", b"second");
    let first = fixture
        .project
        .immutable_handles
        .get(&first_binding, &context)
        .unwrap();
    let error = fixture
        .project
        .immutable_handles
        .get(&second_binding, &context)
        .unwrap_err();
    assert_eq!(
        file_descriptor_error(&error),
        Some(FileDescriptorError::BudgetExceeded {
            requested: 1,
            available: 0,
            limit: 1,
        })
    );
    let mut bytes = [0; 5];
    crate::io::read_exact_at(&first, &mut bytes, 0).unwrap();
    assert_eq!(&bytes, b"first");
    assert_eq!(fixture.project.metrics().open, 1);
    drop(first);
    let second = fixture
        .project
        .immutable_handles
        .get(&second_binding, &context)
        .unwrap();
    assert_eq!(fixture.project.metrics().cache_evictions, 1);
    drop(second);
}

#[test]
fn internal_admission_overrides_strict_capacity_and_restores_foreground_policy() {
    let fixture = Fixture::new(1);
    let context = fixture.project.io_context();
    let first_binding = fixture.binding("first", b"first");
    let second_binding = fixture.binding("second", b"second");
    let third_binding = fixture.binding("third", b"third");
    let fourth_binding = fixture.binding("fourth", b"fourth");
    let first = fixture
        .project
        .immutable_handles
        .get(&first_binding, &context)
        .unwrap();
    assert!(fixture.project.reserve(1).is_err());
    let (second, third) = {
        let _internal = context.internal_operation().unwrap();
        let reservation = fixture.project.reserve(2).unwrap();
        let _nested = fixture.project.reserve_admission(3).unwrap();
        let operation = context.for_operation();
        let second = fixture
            .project
            .immutable_handles
            .get(&second_binding, &operation)
            .unwrap();
        let handles = Arc::clone(&fixture.project.immutable_handles);
        let third = std::thread::spawn(move || {
            handles
                .get(&third_binding, &operation.for_operation())
                .unwrap()
        })
        .join()
        .unwrap();
        assert!(fixture.project.metrics().open > fixture.project.metrics().limit);
        drop(reservation);
        (second, third)
    };
    assert_eq!(fixture.project.metrics().reserved, 0);
    let error = fixture
        .project
        .immutable_handles
        .get(&fourth_binding, &context)
        .unwrap_err();
    assert!(matches!(
        file_descriptor_error(&error),
        Some(FileDescriptorError::BudgetExceeded { .. })
    ));
    assert_eq!(fixture.project.metrics().cached_handles, 3);
    drop((first, second, third));
    fixture.project.immutable_handles.evict_idle(usize::MAX);
    assert_eq!(fixture.project.metrics().open, 0);
}

#[test]
fn wal_and_ownership_handles_are_non_strict_and_remain_counted() {
    let fixture = Fixture::new(1);
    let context = fixture.project.io_context();
    let held = fixture.project.reserve(1).unwrap();
    let wal = context.acquire(DescriptorKind::MutableWal).unwrap();
    let lock = context.acquire(DescriptorKind::OwnershipLock).unwrap();
    assert_eq!(fixture.project.metrics().open, 2);
    assert_eq!(fixture.project.metrics().reserved, 1);
    assert!(context.acquire(DescriptorKind::Transient).is_err());
    drop((held, wal, lock));
    assert_eq!(fixture.project.metrics().open, 0);
    assert_eq!(fixture.project.metrics().reserved, 0);
}

#[test]
fn default_capacity_is_soft_and_existing_domains_preserve_their_policy() {
    assert!(!crate::config::WalReplayConfig::default().strict_capacity_limit);
    let fixture = Fixture::with_capacity_limit(1, false);
    let shared = ProjectFileDescriptors::acquire_existing(&fixture.root, 1).unwrap();
    assert!(!shared.metrics().strict_capacity_limit);
    let held = shared.reserve(2).unwrap();
    assert_eq!(fixture.project.metrics().reserved, 2);
    assert!(
        ProjectFileDescriptors::acquire_existing_with_capacity_limit(&fixture.root, 1, true)
            .is_err()
    );
    assert_eq!(fixture.project.metrics().reserved, 2);
    drop(held);
    assert_eq!(fixture.project.metrics().reserved, 0);
}

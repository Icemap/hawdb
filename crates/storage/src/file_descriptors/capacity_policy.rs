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
    let missing = second.root.join("missing");
    // Initialize native error-message resources outside the measured engine
    // path. Windows may retain process-owned state on the first formatted error.
    drop(std::fs::canonicalize(&missing).unwrap_err().to_string());
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
        assert!(ProjectFileDescriptors::acquire_existing(&missing, 1).is_err());
        assert_eq!(first.project.metrics().open, before.open);
        assert_eq!(first.project.metrics().reserved, before.reserved);
        assert_eq!(first.project.metrics().high_water, before.high_water);
        assert_eq!(super::native_descriptor_count(), native_before);
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
    assert_eq!(
        ProjectFileDescriptors::acquire_existing_with_capacity_limit(&fixture.root, 1, true)
            .unwrap_err(),
        super::HawDBError::FileDescriptors(FileDescriptorError::CapacityPolicyConflict {
            configured: false,
            requested: true,
        })
    );
    assert!(!fixture.project.metrics().strict_capacity_limit);
    assert_eq!(fixture.project.metrics().reserved, 2);
    drop(held);
    assert_eq!(fixture.project.metrics().reserved, 0);
}

#[test]
fn conflicting_capacity_policy_is_typed_without_changing_active_owners() {
    for configured in [false, true] {
        let fixture = Fixture::with_capacity_limit(2, configured);
        let held = fixture.project.reserve(2).unwrap();
        let before = fixture.project.metrics();
        for create in [false, true] {
            let result = if create {
                ProjectFileDescriptors::acquire_with_capacity_limit(&fixture.root, 2, !configured)
            } else {
                ProjectFileDescriptors::acquire_existing_with_capacity_limit(
                    &fixture.root,
                    2,
                    !configured,
                )
            };
            let error = result.unwrap_err();
            assert_eq!(
                file_descriptor_error(&error),
                Some(FileDescriptorError::CapacityPolicyConflict {
                    configured,
                    requested: !configured,
                })
            );
            assert_eq!(fixture.project.metrics(), before);
        }
        let inherited = ProjectFileDescriptors::acquire_existing(&fixture.root, 2).unwrap();
        assert_eq!(inherited.metrics(), before);
        drop(held);
        assert_eq!(fixture.project.metrics().open, 0);
        assert_eq!(fixture.project.metrics().reserved, 0);
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[test]
fn soft_and_internal_capacity_preserve_os_headroom_and_prepaid_slots() {
    const NAME: &str = "file_descriptors::tests::capacity_policy::soft_and_internal_capacity_preserve_os_headroom_and_prepaid_slots";
    if super::run_descriptor_child(NAME) {
        return;
    }
    let original = super::descriptor_os_limits();
    let lowered = libc::rlimit {
        rlim_cur: 256,
        rlim_max: original.rlim_max,
    };
    // SAFETY: Only this isolated child lowers its own soft limit.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lowered) }, 0);
    let allowance = 256 - super::super::os_limit::HOST_HEADROOM;
    for strict in [false, true] {
        let fixture = Fixture::with_capacity_limit(2, strict);
        let context = fixture.project.io_context();
        let path = fixture.root.join("evidence");
        std::fs::write(&path, b"complete committed bytes").unwrap();
        let baseline = fixture.project.metrics();
        let operation = strict.then(|| context.internal_operation().unwrap());
        let prepaid = fixture.project.reserve(3).unwrap();
        let captured = context.for_operation();
        let project = fixture.project.clone();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let competitor = std::thread::spawn(move || {
            let _internal = project.io_context().internal_operation().unwrap();
            let _competing = project.reserve(allowance - 3).unwrap();
            ready_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        ready_rx.recv().unwrap();
        assert_eq!(fixture.project.metrics().reserved, allowance);
        let rejected = |error: std::io::Error| {
            assert_eq!(
                file_descriptor_error(&error),
                Some(FileDescriptorError::OsLimit {
                    requested: 1,
                    os_code: None,
                    soft: Some(256),
                    hard: Some(original.rlim_max),
                })
            );
        };
        rejected(fixture.project.reserve(1).unwrap_err());
        let independent_project = fixture.project.clone();
        rejected(
            std::thread::spawn(move || {
                let independent = independent_project.io_context();
                let _internal = independent.internal_operation().unwrap();
                independent.acquire(DescriptorKind::Transient).unwrap_err()
            })
            .join()
            .unwrap(),
        );
        let file = captured
            .open(crate::file_io::OpenOptions::new().read(true), &path)
            .unwrap();
        assert_eq!(fixture.project.metrics().open, baseline.open + 1);
        assert_eq!(
            fixture.project.metrics().open + fixture.project.metrics().reserved,
            allowance
        );
        let mut bytes = [0; 24];
        crate::io::read_exact_at(&file, &mut bytes, 0).unwrap();
        assert_eq!(&bytes, b"complete committed bytes");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        drop(file);
        release_tx.send(()).unwrap();
        competitor.join().unwrap();
        drop((prepaid, operation));
        let after = fixture.project.metrics();
        assert_eq!(after.open, baseline.open);
        assert_eq!(after.reserved, baseline.reserved);
        assert_eq!(after.high_water, allowance);
        assert_eq!(after.os_limit_rejections, baseline.os_limit_rejections + 2);
        assert_eq!(after.budget_rejections, baseline.budget_rejections);
        assert_eq!(super::descriptor_os_limits().rlim_cur, 256);
        if strict {
            assert!(fixture.project.reserve(3).is_err());
        }
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[test]
fn projects_share_process_capacity_and_cannot_steal_prepaid_slots() {
    const NAME: &str = "file_descriptors::tests::capacity_policy::projects_share_process_capacity_and_cannot_steal_prepaid_slots";
    if super::run_descriptor_child(NAME) {
        return;
    }
    let original = super::descriptor_os_limits();
    let lowered = libc::rlimit {
        rlim_cur: 96,
        rlim_max: original.rlim_max,
    };
    // SAFETY: This isolated child changes only its own soft limit.
    assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_NOFILE, &lowered) }, 0);
    let allowance = 96 - super::super::os_limit::HOST_HEADROOM;
    let mut options = crate::file_io::OpenOptions::new();
    options.read(true);
    let rejected = |error: std::io::Error| {
        assert_eq!(
            file_descriptor_error(&error),
            Some(FileDescriptorError::OsLimit {
                requested: 1,
                os_code: None,
                soft: Some(96),
                hard: Some(original.rlim_max),
            })
        );
    };
    for strict in [false, true] {
        let fixtures = (0..3)
            .map(|_| Fixture::with_capacity_limit(4, strict))
            .collect::<Vec<_>>();
        let bytes = b"shared process budget";
        for fixture in &fixtures {
            std::fs::write(fixture.root.join("evidence"), bytes).unwrap();
        }
        let standalone = fixtures[0].root.with_extension("standalone");
        std::fs::write(&standalone, bytes).unwrap();
        let standalone_context = super::super::FileOpenContext::for_path(&standalone).unwrap();
        let scopes = fixtures
            .iter()
            .map(|fixture| {
                strict.then(|| fixture.project.io_context().internal_operation().unwrap())
            })
            .collect::<Vec<_>>();
        let baseline = super::native_descriptor_count();
        let prepaid = fixtures[0].project.reserve(2).unwrap();
        let captured = fixtures[0].project.io_context();
        let mut held = (0..allowance - 2)
            .map(|index| {
                let fixture = &fixtures[1 + index % 2];
                fixture
                    .project
                    .io_context()
                    .open(&options, &fixture.root.join("evidence"))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            fixtures
                .iter()
                .map(|fixture| {
                    let m = fixture.project.metrics();
                    m.open + m.reserved
                })
                .sum::<usize>(),
            allowance
        );
        for fixture in &fixtures {
            rejected(fixture.project.reserve(1).unwrap_err());
            assert_eq!(fixture.project.metrics().budget_rejections, 0);
            assert_eq!(fixture.project.metrics().strict_capacity_limit, strict);
        }
        rejected(standalone_context.open(&options, &standalone).unwrap_err());
        let new_root = fixtures[0].root.with_extension("new-project");
        let error =
            ProjectFileDescriptors::acquire_with_capacity_limit(&new_root, 4, strict).unwrap_err();
        assert_eq!(
            file_descriptor_error(&error),
            Some(FileDescriptorError::OsLimit {
                requested: 1,
                os_code: None,
                soft: Some(96),
                hard: Some(original.rlim_max),
            })
        );
        assert!(!new_root.exists());
        let mut protected = (0..2)
            .map(|_| {
                captured
                    .open(&options, &fixtures[0].root.join("evidence"))
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(super::native_descriptor_count(), baseline + allowance);
        // The kernel still has host headroom even though engine admission is full.
        drop(std::fs::File::open("/dev/null").unwrap());
        let mut actual = vec![0; bytes.len()];
        crate::io::read_exact_at(&protected[0], &mut actual, 0).unwrap();
        assert_eq!(actual, bytes);
        drop(protected.pop());
        assert_eq!(fixtures[0].project.metrics().reserved, 1);
        rejected(fixtures[2].project.reserve(1).unwrap_err());
        protected.push(
            captured
                .open(&options, &fixtures[0].root.join("evidence"))
                .unwrap(),
        );
        drop(prepaid);
        drop(protected.pop());
        assert_eq!(fixtures[0].project.metrics().reserved, 0);
        let start = Arc::new(std::sync::Barrier::new(3));
        let (result_tx, result_rx) = std::sync::mpsc::channel();
        let workers = fixtures[1..]
            .iter()
            .map(|fixture| {
                let project = fixture.project.clone();
                let path = fixture.root.join("evidence");
                let start = Arc::clone(&start);
                let result_tx = result_tx.clone();
                let (release_tx, release_rx) = std::sync::mpsc::channel();
                let worker = std::thread::spawn(move || {
                    let context = project.io_context();
                    let _internal = context.internal_operation().unwrap();
                    let mut options = crate::file_io::OpenOptions::new();
                    options.read(true);
                    start.wait();
                    let file = context.open(&options, &path);
                    result_tx
                        .send(
                            file.as_ref()
                                .map(|_| ())
                                .map_err(|error| file_descriptor_error(error)),
                        )
                        .unwrap();
                    let _ = release_rx.recv();
                    drop(file);
                });
                (worker, release_tx)
            })
            .collect::<Vec<_>>();
        start.wait();
        let results = (0..2)
            .map(|_| result_rx.recv().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(
                    result,
                    Err(Some(FileDescriptorError::OsLimit {
                        os_code: None,
                        soft: Some(96),
                        ..
                    }))
                ))
                .count(),
            1
        );
        assert_eq!(
            fixtures
                .iter()
                .map(|fixture| {
                    let metrics = fixture.project.metrics();
                    metrics.open + metrics.reserved
                })
                .sum::<usize>(),
            allowance
        );
        for (_, release) in &workers {
            release.send(()).unwrap();
        }
        for (worker, _) in workers {
            worker.join().unwrap();
        }
        let missing = fixtures[1]
            .project
            .io_context()
            .open(&options, &fixtures[1].root.join("missing"))
            .unwrap_err();
        assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);
        // A failed native open returns the shared slot for another domain.
        held.push(
            fixtures[2]
                .project
                .io_context()
                .open(&options, &fixtures[2].root.join("evidence"))
                .unwrap(),
        );
        rejected(fixtures[1].project.reserve(1).unwrap_err());
        assert_eq!(super::native_descriptor_count(), baseline + allowance);
        drop((protected, held));
        assert_eq!(super::native_descriptor_count(), baseline);
        for fixture in &fixtures {
            assert_eq!(fixture.project.metrics().open, 0);
            assert_eq!(fixture.project.metrics().reserved, 0);
        }
        let all_credit = fixtures[1].project.reserve(allowance).unwrap();
        assert_eq!(fixtures[1].project.metrics().reserved, allowance);
        drop(all_credit);
        drop(scopes);
        let reopened =
            ProjectFileDescriptors::acquire_with_capacity_limit(&new_root, 4, strict).unwrap();
        assert_eq!(reopened.metrics().open, 0);
        drop(reopened);
        std::fs::remove_dir(&new_root).unwrap();
        std::fs::remove_file(standalone).unwrap();
    }
}

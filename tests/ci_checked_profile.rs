// Copyright 2026 Nowledge
// SPDX-License-Identifier: Apache-2.0

#[test]
fn debug_assertions_remain_enabled() {
    assert!(std::hint::black_box(cfg!(debug_assertions)));
}

#[test]
#[should_panic(expected = "debug assertion remains active")]
fn debug_assertions_execute_in_optimized_ci() {
    debug_assert!(
        std::hint::black_box(false),
        "debug assertion remains active"
    );
}

#[test]
#[should_panic(expected = "attempt to add with overflow")]
fn integer_overflow_remains_checked() {
    let value = std::hint::black_box(u8::MAX);
    let _ = std::hint::black_box(value + 1);
}

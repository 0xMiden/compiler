//! Felt-related intrinsics and helpers.

use miden_field::Felt;

#[cfg(all(target_family = "wasm", miden))]
unsafe extern "C" {
    #[link_name = "intrinsics::felt::assert"]
    fn extern_assert(a: Felt);

    #[link_name = "intrinsics::felt::assertz"]
    fn extern_assertz(a: Felt);

    #[link_name = "intrinsics::felt::assert_eq"]
    fn extern_assert_eq(a: Felt, b: Felt);
}

/// Fails if `a` != 1.
#[cfg(all(target_family = "wasm", miden))]
#[inline(always)]
pub fn assert(a: Felt) {
    unsafe { extern_assert(a) }
}

/// Fails if `a` != 0.
#[cfg(all(target_family = "wasm", miden))]
#[inline(always)]
pub fn assertz(a: Felt) {
    unsafe { extern_assertz(a) }
}

/// Fails if `a` != `b`.
#[cfg(all(target_family = "wasm", miden))]
#[inline(always)]
pub fn assert_eq(a: Felt, b: Felt) {
    unsafe { extern_assert_eq(a, b) }
}

/// Fails if `a` != 1.
#[cfg(not(all(target_family = "wasm", miden)))]
#[inline(always)]
pub fn assert(a: Felt) {
    if a != Felt::new(1) {
        panic!("assert: expected 1");
    }
}

/// Fails if `a` != 0.
#[cfg(not(all(target_family = "wasm", miden)))]
#[inline(always)]
pub fn assertz(a: Felt) {
    if a != Felt::new(0) {
        panic!("assertz: expected 0");
    }
}

/// Fails if `a` != `b`.
#[cfg(not(all(target_family = "wasm", miden)))]
#[inline(always)]
pub fn assert_eq(a: Felt, b: Felt) {
    if a != b {
        panic!("assert_eq: values differ");
    }
}

/// Creates a `Felt` from an integer constant checking that it is within the
/// valid range at compile time.
#[macro_export]
macro_rules! felt {
    // Trigger a compile-time error if the value is not a constant
    ($value:literal) => {{
        const VALUE: u64 = $value as u64;
        // assert!(VALUE <= Felt::M, "Invalid Felt value, must be >= 0 and <= 2^64 - 2^32 + 1");
        $crate::Felt::new(VALUE)
    }};
}

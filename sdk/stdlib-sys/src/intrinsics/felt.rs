pub use miden_felt::{Felt, FeltError, assert, assert_eq, assertz};

/// Creates a `Felt` from an integer constant checking that it is within the
/// valid range at compile time.
#[macro_export]
macro_rules! felt {
    // Trigger a compile-time error if the value is not a constant
    ($value:literal) => {{
        const VALUE: u64 = $value as u64;
        // assert!(VALUE <= Felt::M, "Invalid Felt value, must be >= 0 and <= 2^64 - 2^32 + 1");
        // Temporarily switch to `from_u32` to use `bitcast` and avoid checks.
        // see https://github.com/0xMiden/compiler/issues/361
        assert!(VALUE <= u32::MAX as u64, "Invalid value, must be >= 0 and <= 2^32");
        const VALUE_U32: u32 = $value as u32;
        $crate::Felt::from_u32(VALUE_U32)
    }};
}

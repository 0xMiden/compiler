//! This module contains the set of compiler-emitted assertion codes, along with their explanations

/// This assertion fails when a pointer address does not meet minimum alignment for the type
pub const ASSERT_FAILED_ALIGNMENT: u32 = 0xfa;

use std::borrow::Cow;

use miden_debug::{FromMidenRepr, ToMidenRepr};
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, Felt, Immediate, PointerType, SourceSpan, Type, ValueRef,
    dialects::builtin::{BuiltinOpBuilder, attributes::Signature},
};
use proptest::{
    prelude::{Strategy, any},
    prop_assert_eq,
    test_runner::{TestCaseError, TestError, TestRunner},
};

use crate::testing::*;

mod load_bool;
mod load_dw;
mod load_qw;
mod load_sw;
mod load_u16;
mod load_u64_unaligned;
mod load_u8;
mod regressions;
mod store_qw;
mod store_u16;
mod store_u32_unaligned;
mod store_u64_unaligned;
mod store_u8;

/// Generates a random word-aligned byte address suitable for memory tests.
///
/// The address is guaranteed to be above the 16 pages reserved for the Rust stack
/// (i.e. in pages 17..256), and aligned to a 4-byte boundary.
pub fn random_word_aligned_addr() -> impl Strategy<Value = u32> {
    // Page 17..256, word offset 0..1024 within that page
    (17u32..256, 0u32..1024)
        .prop_map(|(page, word)| ((page * u16::MAX as u32) + (word * 4)).next_multiple_of(16))
}

/// Enables test helpers generic over 128 bit integer types.
pub trait QuadwordIO: FromMidenRepr + PartialEq + Clone + std::fmt::Debug {
    fn hir_type() -> Type;
    fn from_le_bytes(bytes: [u8; 16]) -> Self;
    fn to_le_bytes(&self) -> [u8; 16];
    fn as_immediate(&self) -> Immediate;
}

impl QuadwordIO for i128 {
    fn hir_type() -> Type {
        Type::I128
    }

    fn from_le_bytes(bytes: [u8; 16]) -> Self {
        i128::from_le_bytes(bytes)
    }

    fn to_le_bytes(&self) -> [u8; 16] {
        i128::to_le_bytes(*self)
    }

    fn as_immediate(&self) -> Immediate {
        Immediate::I128(*self)
    }
}

impl QuadwordIO for u128 {
    fn hir_type() -> Type {
        Type::U128
    }

    fn from_le_bytes(bytes: [u8; 16]) -> Self {
        u128::from_le_bytes(bytes)
    }

    fn to_le_bytes(&self) -> [u8; 16] {
        u128::to_le_bytes(*self)
    }

    fn as_immediate(&self) -> Immediate {
        Immediate::U128(*self)
    }
}

use std::borrow::Cow;

use miden_debug::ToMidenRepr;
use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, Felt, PointerType, SourceSpan, Type, ValueRef,
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
mod load_sw;
mod load_u16;
mod load_u64_unaligned;
mod load_u8;
mod regressions;
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

//! Basic integer arithmetic, bitwise, bit-counting, and unsigned widening cases.

use super::super::harness::run_case;

#[test]
fn add() {
    run_case("add", include_str!("../cases/case_add.rs"));
}

#[test]
fn xor() {
    run_case("xor", include_str!("../cases/case_xor.rs"));
}

/// Non-commutative — exercises argument ordering (`input1 - input2`).
#[test]
fn sub() {
    run_case("sub", include_str!("../cases/case_sub.rs"));
}

#[test]
fn muladd() {
    run_case("muladd", include_str!("../cases/case_muladd.rs"));
}

/// Exercises integer width conversions and per-width bit-counting arms in
/// `codegen/masm/src/emit/unary.rs` (`!x` lowers to xor, never `bnot`).
#[test]
fn widening() {
    run_case("widening", include_str!("../cases/case_widening.rs"));
}

/// Exercises u32 bitwise / shift / rotate / comparison emitter arms in
/// `codegen/masm/src/emit/binary.rs`.
#[test]
fn bitops() {
    run_case("bitops", include_str!("../cases/case_bitops.rs"));
}

/// `i64.mul_wide_u` with a constant multiplicand (reaches `Zext::fold`'s
/// U128 success arm) plus first genuine `i32.ctz`/`i64.ctz` uses.
#[test]
fn zext_wide_ctz() {
    run_case("zext_wide_ctz", include_str!("../cases/case_zext_wide_ctz.rs"));
}

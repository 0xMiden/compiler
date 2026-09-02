//! Basic integer arithmetic, bitwise, bit-counting, and unsigned widening cases.

use super::super::harness::{run_case, run_case_with_inputs};

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

/// Constant-divisor forms next to generic division: on wasm LLVM keeps
/// `x / C` and `x % C` as `div_s/div_u/rem_s/rem_u` with an immediate
/// (only UNSIGNED powers of two become `shr_u`/`and` — no magic-multiply
/// forms exist on this target), so the immediate-operand divisions of
/// 8/16/7/10/32/1000/641/2^32+1/3/2^63 are folded together with the same
/// quotients over opaquely-equal runtime divisors (checked_div/checked_mod
/// emitters) on i32/u32/i64/u64 with fully dynamic dividends.
#[test]
fn div_const_forms() {
    run_case("div_const_forms", include_str!("../cases/case_div_const_forms.rs"));
}

/// Shift/rotate shapes at count boundaries: checked_shl/checked_shr/
/// overflowing_shl/overflowing_shr on u32/i32/u64/i64 (LLVM's `count <
/// width` compare + select around the masked shift), sub-word u8/u16/i8/i16
/// wrapping shifts and rotates (count masked % 8 / % 16, shifted as i32 and
/// re-masked), and u128 rotate_left/rotate_right by a runtime count (funnel
/// shift over the __ashlti3/__lshrti3 libcalls, masked % 128).
#[test]
fn shift_shapes() {
    run_case("shift_shapes", include_str!("../cases/case_shift_shapes.rs"));
}

/// Pinned edge grid for `shift_shapes`: count (input2) at 0, 1, 7, 8, 15, 16,
/// 31, 32, 33, 63, 64, 65, 127, 128, 129 and u32::MAX on the value
/// 0x80000001 (sign bit and low bit set at every width), plus all-ones /
/// MAX / 1 / 0x80 / 0 values at the width-1, 2*width-1 and 128 counts.
#[test]
fn shift_shapes_edges() {
    run_case_with_inputs(
        "shift_shapes_edges",
        include_str!("../cases/case_shift_shapes.rs"),
        &[
            (0x80000001, 0),
            (0x80000001, 1),
            (0x80000001, 7),
            (0x80000001, 8),
            (0x80000001, 15),
            (0x80000001, 16),
            (0x80000001, 31),
            (0x80000001, 32),
            (0x80000001, 33),
            (0x80000001, 63),
            (0x80000001, 64),
            (0x80000001, 65),
            (0x80000001, 127),
            (0x80000001, 128),
            (0x80000001, 129),
            (0x80000001, 0xffffffff),
            (0xffffffff, 31),
            (0x7fffffff, 63),
            (1, 127),
            (0x80, 7),
            (0, 64),
        ],
    );
}

/// Bit-manipulation shapes at width boundaries: swap_bytes / reverse_bits
/// (LLVM shift/mask expansions) on u32/u64/u128, is_power_of_two,
/// checked_next_power_of_two, leading_ones/trailing_ones, and wrapping_abs /
/// unsigned_abs / checked_abs / signum on i32/i64/i128 at MIN.
#[test]
fn bit_shapes() {
    run_case("bit_shapes", include_str!("../cases/case_bit_shapes.rs"));
}

/// Pinned edge grid for `bit_shapes`: (0x80000000, 0) makes i32::MIN,
/// i64::MIN and i128::MIN at once; all-zero and all-ones rows; single bits
/// at 0/16/30/31/32/63 (is_power_of_two, next_power_of_two at the top bit
/// and just below it); byte-reversal palindromes; MAX rows.
#[test]
fn bit_shapes_edges() {
    run_case_with_inputs(
        "bit_shapes_edges",
        include_str!("../cases/case_bit_shapes.rs"),
        &[
            (0, 0),
            (0xffffffff, 0xffffffff),
            (0x80000000, 0),
            (0, 0x80000000),
            (1, 0),
            (0, 1),
            (0x7fffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x00010000, 0),
            (0xffff0000, 0x0000ffff),
            (0x80000001, 0),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x40000000, 0),
            (2, 0),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Overflow-detecting multiplication at MIN/MAX: overflowing_mul /
/// checked_mul / saturating_mul on u64 (mul_wide_u + hi-word test) and on
/// u32/i32 (i64 products + range compares), plus abs_diff, midpoint and
/// u64/i64 checked_add/checked_sub None arms — all LLVM-legalized into
/// wrapping ops + compares. The i64 overflow-checked multiplies are pinned
/// separately as the ignored `sat_mul_i64`/`pow_i64` guest-LLVM miscompile.
#[test]
fn ovf_mul() {
    run_case("ovf_mul", include_str!("../cases/case_ovf_mul.rs"));
}

/// Pinned edge grid for `ovf_mul`: (MAX, MAX) is u64::MAX * u64::MAX;
/// (0x80000000, 0x80000000) is 2^63 * 2^63; (0x80000000, 0) and
/// (0x80000000, 2) are 2^63 * 2 (the first overflowing product) and
/// 2^63 * 2^33; (0x80000000, MAX) is |i64::MIN - i64::MAX| == 2^64-1; zero /
/// one identities; i32::MIN * -1 rows for the 32-bit forms; mixed rows.
#[test]
fn ovf_mul_edges() {
    run_case_with_inputs(
        "ovf_mul_edges",
        include_str!("../cases/case_ovf_mul.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x80000000, 0),
            (0x80000000, 1),
            (0x80000000, 2),
            (0, 0),
            (1, 1),
            (0x7fffffff, 0x7fffffff),
            (0x7fffffff, 0xffffffff),
            (1, 0xffffffff),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Integer logarithm / root / power helpers at value boundaries:
/// checked_ilog2 / checked_ilog10 / checked_ilog(3) on u32/u64/u128, isqrt,
/// checked_pow with a small dynamic exponent, and u32/u64/i64/i128 abs_diff.
#[test]
fn int_logs() {
    run_case("int_logs", include_str!("../cases/case_int_logs.rs"));
}

/// Pinned edge grid for `int_logs`: 0 (every checked log None), 1, the
/// exact powers 9/10, 99/100, 10^9, 2^32-1 / 2^32 (u64 rows via input2),
/// perfect squares +-1 (65535/65536 -> isqrt boundary), MAX rows, and
/// exponents 0..7 through input2 & 7.
#[test]
fn int_logs_edges() {
    run_case_with_inputs(
        "int_logs_edges",
        include_str!("../cases/case_int_logs.rs"),
        &[
            (0, 0),
            (1, 0),
            (9, 10),
            (99, 100),
            (1000000000, 3),
            (0xffffffff, 0xffffffff),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (65535, 16),
            (65536, 65535),
            (1, 0xffffffff),
            (0x80000000, 0),
            (0, 0x80000000),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Pinned edge grid for `div_const_forms`: i32/i64 MIN and -1 dividends
/// (the sign-bias fixup of the shift forms and the sign correction of the
/// magic forms), 0, MAX, all-ones u64, and the u64 limb-boundary values
/// 2^32-1 / 2^32 / 2^63 against the 2^32 and 2^63 power-of-two divisors.
#[test]
fn div_const_forms_edges() {
    run_case_with_inputs(
        "div_const_forms_edges",
        include_str!("../cases/case_div_const_forms.rs"),
        &[
            (0x80000000, 0),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0xffffffff),
            (0, 0),
            (0x7fffffff, 0x7fffffff),
            (1, 1),
            (0, 0x80000000),
            (0xffffffff, 0),
            (0, 1),
            (1, 0),
            (0x80000000, 0x80000000),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

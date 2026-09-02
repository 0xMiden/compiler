//! u64/u128/i128 runtime arithmetic through wide-arithmetic ops and compiler-builtins.

use super::super::harness::{run_case, run_case_with_inputs};

/// u64-returning helper with early returns, trap exit, and loop exit —
/// multi-word successor operands through branch lowering.
#[test]
fn u64_exits() {
    run_case("u64_exits", include_str!("../cases/case_u64_exits.rs"));
}

/// u128 arithmetic feeding branch conditions — wide-arithmetic wasm ops
/// (add128/sub128/mul_wide) and their lowering.
#[test]
fn u128_mix() {
    run_case("u128_mix", include_str!("../cases/case_u128_mix.rs"));
}

/// Unsigned u64 comparisons (branches + select), dynamic-count rotates, and
/// u64 leading_zeros — exercises the `lt/lte/gt/gte_u64`, `rotr_u64`, and u64
/// `clz` emitter arms.
#[test]
fn u64_ucmp() {
    run_case("u64_ucmp", include_str!("../cases/case_u64_ucmp.rs"));
}

/// Non-strict unsigned comparisons materialized as VALUES via
/// `#[inline(never)]` helpers — the only producer of `i64.ge_u` (the
/// `gte_u64` emitter arm); branch/select position is always canonicalized
/// to strict compares.
#[test]
fn ucmp_ge() {
    run_case("ucmp_ge", include_str!("../cases/case_ucmp_ge.rs"));
}

/// Unsigned u64 division/remainder with dynamic non-zero divisors —
/// `checked_div_u64`/`checked_mod_u64` emitter arms (miden-core-lib
/// `u64::div`/`u64::mod`).
#[test]
fn u64_udiv() {
    run_case("u64_udiv", include_str!("../cases/case_u64_udiv.rs"));
}

/// u128 `/` with dynamic small (u64-range) and full-width non-zero divisors —
/// executes compiler-builtins `__udivti3`/`u128_div_rem` (u64 clz/shift/
/// subtract long-division loops compiled into the guest) on the VM.
#[test]
fn u128_udiv() {
    run_case("u128_udiv", include_str!("../cases/case_u128_udiv.rs"));
}

/// Pinned edge grid for `u128_udiv`: divisor exactly 1 with a huge dividend
/// ((1, 0) makes b == 1), dividend 0 ((0, 0)), smallest divisor > dividend
/// ((0, x) makes a == 0 so q1 divides n by n+1), u64::MAX and high-bit-set
/// small divisors. Divisor == dividend and both-limbs-max are outside this
/// derivation's range — pinned by `u128_bounds_edges` instead.
#[test]
fn u128_udiv_edges() {
    run_case_with_inputs(
        "u128_udiv_edges",
        include_str!("../cases/case_u128_udiv.rs"),
        &[
            (0, 0),
            (1, 0),
            (0, 1),
            (0, 0xffffffff),
            (0xffffffff, 0xffffffff),
            (0xffffffff, 0),
            (1, 0xffffffff),
            (2, 0),
            (0x80000000, 0),
            (3, 5),
        ],
    );
}

/// u128 `%` with dynamic small and full-width non-zero divisors — executes
/// compiler-builtins `__umodti3` remainder paths on the VM.
#[test]
fn u128_umod() {
    run_case("u128_umod", include_str!("../cases/case_u128_umod.rs"));
}

/// Pinned edge grid for `u128_umod`: dividend 0 ((0, 0)), a full-width
/// divisor greater than the dividend ((0, 1): swapped-limb d2 has high limb
/// K > a), and high-bit-set small divisors ((0xFFFFFFFF, 0): a|1 ==
/// 0xFFFFFFFF00000001). Divisor 1 with a nonzero dividend and divisor ==
/// dividend are outside this derivation's range — pinned by
/// `u128_bounds_edges` instead.
#[test]
fn u128_umod_edges() {
    run_case_with_inputs(
        "u128_umod_edges",
        include_str!("../cases/case_u128_umod.rs"),
        &[
            (0, 0),
            (0, 1),
            (1, 0),
            (0xffffffff, 0),
            (0xffffffff, 0xffffffff),
            (0, 0xffffffff),
            (0x80000000, 1),
            (5, 3),
            (2, 7),
            (123456789, 987654321),
        ],
    );
}

/// u128 `/` and `%` boundary relations unreachable from the u128_udiv/
/// u128_umod input derivations: divisor == dividend exactly (n | 1 on odd n),
/// smallest divisor > dividend (even n), both-limbs-max operands, and
/// divisor 1 with a nonzero dividend — `/` and `%` use limb-swapped operand
/// pairs so the same-pair div+rem mul-sub fusion cannot elide either builtin.
#[test]
fn u128_bounds() {
    run_case("u128_bounds", include_str!("../cases/case_u128_bounds.rs"));
}

/// Pinned edge grid for `u128_bounds`: (MAX, MAX) makes both operands
/// u128::MAX (MAX/MAX == 1, MAX%MAX == 0); (1, 0)/(0, 1) pin odd/even n and
/// m in both orders (divisor == dividend vs == dividend+1, and divisor-1
/// legs on the opposite operation); (0, 0) pins 0/1 and 0%1.
#[test]
fn u128_bounds_edges() {
    run_case_with_inputs(
        "u128_bounds_edges",
        include_str!("../cases/case_u128_bounds.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0, 0),
            (1, 0),
            (0, 1),
            (2, 0),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x80000000, 0x80000000),
            (3, 3),
            (7, 5),
        ],
    );
}

/// i128 `/` with an odd (never-MIN) both-sign numerator and dynamic positive/
/// negative divisors — executes `__divti3`'s sign-fixup around the unsigned
/// division core on the VM.
#[test]
fn i128_sdiv() {
    run_case("i128_sdiv", include_str!("../cases/case_i128_sdiv.rs"));
}

/// i128 `%` with an odd (never-MIN) both-sign numerator and dynamic positive/
/// negative divisors — executes `__modti3` (truncate-toward-zero remainder
/// signs) on the VM.
#[test]
fn i128_srem() {
    run_case("i128_srem", include_str!("../cases/case_i128_srem.rs"));
}

/// Dynamic u128 `<<`/`>>` with counts in [0, 128) — executes the
/// compiler-builtins `__ashlti3`/`__lshrti3` two-limb funnel shifts (both
/// count < 64 and >= 64 legs) on the VM.
#[test]
fn u128_shifts() {
    run_case("u128_shifts", include_str!("../cases/case_u128_shifts.rs"));
}

/// Pinned edge grid for `u128_shifts`: both shift counts (left = input2 &
/// 127, right = (input1 ^ input2) & 127) pinned to 0/1/63/64/65/127 (plus a
/// 126 row) — the funnel-shift limb-crossing boundaries of `__ashlti3`/
/// `__lshrti3`; rows with input1 == 0xFF give a byte-splat all-ones high
/// limb.
#[test]
fn u128_shifts_edges() {
    run_case_with_inputs(
        "u128_shifts_edges",
        include_str!("../cases/case_u128_shifts.rs"),
        &[
            (0, 0),
            (1, 0),
            (1, 1),
            (0xff, 0x3f),
            (0x41, 0x01),
            (0x40, 0x40),
            (0, 0x41),
            (0x3f, 0x40),
            (0, 0x7f),
            (0xff, 0x7f),
            (0x3e, 0x01),
            (0xffffffff, 0x40),
            (0xff, 0x01),
        ],
    );
}

/// Dynamic i128 arithmetic `>>` on both-sign values — executes `__ashrti3`
/// including the sign-propagating count >= 64 leg (`i64.shr_s` fills the high
/// limb) on the VM.
#[test]
fn i128_ashr() {
    run_case("i128_ashr", include_str!("../cases/case_i128_ashr.rs"));
}

/// Pinned edge grid for `i128_ashr`: w1's sign is input1 bit 31 and its
/// count is input2 & 127; w2's count is (input1 >> 3) & 127 (bits 3..9,
/// independent of the sign bit). Rows pin counts 0/1/63/64/65/127 on
/// negative AND positive values — count 127 on negative w1 is the full
/// `__ashrti3` sign-fill (result -1).
#[test]
fn i128_ashr_edges() {
    run_case_with_inputs(
        "i128_ashr_edges",
        include_str!("../cases/case_i128_ashr.rs"),
        &[
            (0x80000000, 0),
            (0x80000008, 1),
            (0x800001f8, 63),
            (0x80000200, 64),
            (0x80000208, 65),
            (0x800003f8, 127),
            (0x000003f8, 127),
            (0x00000200, 64),
            (0, 1),
            (0x7ffffff8, 63),
            (0xffffffff, 0xffffffff),
        ],
    );
}

/// u128 `count_ones`/`leading_zeros`/`trailing_zeros` on dynamic values —
/// executes the i64 popcnt limb sum and the clz/ctz limb selects (both legs,
/// via parity-zeroed limbs) on the VM.
#[test]
fn u128_bits() {
    run_case("u128_bits", include_str!("../cases/case_u128_bits.rs"));
}

/// u128 comparisons: branch/select position (strict two-limb lt/gt chains)
/// plus `#[inline(never)]` bool-value `<=`/`==` — executes the 128-bit
/// carry/borrow compare legalization on the VM.
#[test]
fn u128_cmp() {
    run_case("u128_cmp", include_str!("../cases/case_u128_cmp.rs"));
}

/// Wide multiplication and carries at operand boundaries: `i64.mul_wide_u`
/// / `i64.mul_wide_s` products with both hi and lo words folded, 4-limb
/// u128 x u128 products in the core-lib wrapping_mul, a multiply-add chain
/// whose carries ripple through all four limbs, and an i128 product of a
/// negated operand.
#[test]
fn wide_mul_edges() {
    run_case("wide_mul_edges", include_str!("../cases/case_wide_mul_edges.rs"));
}

/// Pinned edge grid for `wide_mul_edges`: (MAX, MAX) is u64::MAX * u64::MAX,
/// -1 * -1 and u128::MAX * u128::MAX; (0x80000000, 0x80000000) is 2^63 *
/// 2^63 and i64::MIN * i64::MIN; (0x80000000, MAX) is i64::MIN * -1 and
/// i64::MIN * i64::MAX; odd `a ^ b` rows select the 2^64 operand for the
/// (2^64-1) * (2^64+1) == MAX product; 0/1 rows pin the identities.
#[test]
fn wide_mul_edges_edges() {
    run_case_with_inputs(
        "wide_mul_edges_edges",
        include_str!("../cases/case_wide_mul_edges.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (1, 1),
            (0, 0),
            (1, 0xffffffff),
            (0xffffffff, 1),
            (0x80000000, 1),
            (1, 0x80000000),
            (0x7fffffff, 0xffffffff),
            (0xffffffff, 0x7fffffff),
            (3, 5),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// 128-bit division/remainder GUARD shapes: i128 checked_div/checked_rem/
/// wrapping_div/wrapping_rem/overflowing_div and u128 checked_div/checked_rem
/// with dynamic divisors reaching 0, -1 and i32::MIN — LLVM's `rhs == 0` /
/// `rhs == -1 && lhs == MIN` branch arms around the `__divti3`/`__modti3`/
/// `__udivti3`/`__umodti3` builtins, which the by-construction-safe divisors
/// of i128_sdiv/i128_srem/u128_udiv/u128_umod never form.
#[test]
fn div128_guards() {
    run_case("div128_guards", include_str!("../cases/case_div128_guards.rs"));
}

/// Pinned edge grid for `div128_guards`: (0x80000000, 0xFFFFFFFF) is exactly
/// i128::MIN / -1 (None / wrapping MIN / rem 0 / overflow flag) and u128 2^127
/// / MAX-splat; (x, 0) divides by zero everywhere; MIN / 1, MIN / i32::MIN,
/// -1 / -1, 0 / -1, small / negative and the all-ones rows.
#[test]
fn div128_guards_edges() {
    run_case_with_inputs(
        "div128_guards_edges",
        include_str!("../cases/case_div128_guards.rs"),
        &[
            (0x80000000, 0xffffffff),
            (0x80000000, 0),
            (0x80000000, 1),
            (0x80000000, 0x80000000),
            (0xffffffff, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0xffffffff),
            (0, 0xffffffff),
            (0, 0),
            (1, 0x80000000),
            (7, 0xfffffffe),
            (0xfffffff9, 2),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// 128-bit shift shapes at count boundaries: u128/i128 checked_shl/
/// checked_shr/overflowing_shl/overflowing_shr (compare + select around the
/// __ashlti3/__lshrti3/__ashrti3 libcalls), i128 wrapping shifts of negative
/// values across the 64-bit limb boundary, u8/u16/i8 checked shifts, and u64
/// shifts with a u64-typed count.
#[test]
fn shift128_shapes() {
    run_case("shift128_shapes", include_str!("../cases/case_shift128_shapes.rs"));
}

/// Pinned edge grid for `shift128_shapes`: counts 0, 1, 7, 8, 15, 16, 31, 32,
/// 63, 64, 65, 127, 128, 129 and u32::MAX on a negative (0x80000001-built)
/// value, plus MAX/1/0 values at 64 and 128.
#[test]
fn shift128_shapes_edges() {
    run_case_with_inputs(
        "shift128_shapes_edges",
        include_str!("../cases/case_shift128_shapes.rs"),
        &[
            (0x80000001, 0),
            (0x80000001, 1),
            (0x80000001, 7),
            (0x80000001, 8),
            (0x80000001, 15),
            (0x80000001, 16),
            (0x80000001, 31),
            (0x80000001, 32),
            (0x80000001, 63),
            (0x80000001, 64),
            (0x80000001, 65),
            (0x80000001, 127),
            (0x80000001, 128),
            (0x80000001, 129),
            (0x80000001, 0xffffffff),
            (0xffffffff, 64),
            (1, 128),
            (0, 64),
            (0x7fffffff, 127),
        ],
    );
}

/// Mixed-width expression trees with casts in the middle: u64 and u128
/// products with their high words extracted by shift + truncation, u32 ->
/// u64 -> u128 -> u64 -> u32 round trips, limb swaps, an i64 from a signed
/// high half and unsigned low half, a u128 assembled from four u32 limbs and
/// taken apart again, and carries crossing the 32- and 64-bit limbs.
#[test]
fn width_trees() {
    run_case("width_trees", include_str!("../cases/case_width_trees.rs"));
}

/// Pinned edge grid for `width_trees`: all-ones words (every carry ripples,
/// MAX products), zero, one-sided all-ones, sign-bit-only words (the signed
/// high half at i32::MIN), and mixed rows.
#[test]
fn width_trees_edges() {
    run_case_with_inputs(
        "width_trees_edges",
        include_str!("../cases/case_width_trees.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0, 0),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x80000000, 0x80000000),
            (1, 0xffffffff),
            (0xffffffff, 1),
            (0x80000000, 0),
            (0, 0x80000000),
            (0x7fffffff, 0x80000000),
            (1, 1),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

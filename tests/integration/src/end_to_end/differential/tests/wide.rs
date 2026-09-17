//! u64/u128/i128 runtime arithmetic through wide-arithmetic ops and compiler-builtins.

use super::super::harness::{
    run_case, run_case_with_flags, run_case_with_flags_and_inputs, run_case_with_inputs,
};

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

/// Formerly `#[ignore]`d (F9), the `i64.add128` member of the family:
/// `u128::saturating_add` on dynamic operands. rustc 1.97.0-nightly
/// (c935696dd 2026-04-29, LLVM 22.1.4) emitted the carry test `sum < a` with
/// the `local.get` of the sum's high limb placed BEFORE the `i64.add128` that
/// defines it (WAT: `local.get 4` ... `i64.add128` `local.set 4` `local.tee 5`
/// ... `i64.lt_u` ... `select`), so the wasm compared a zero-initialised local
/// instead of the high limb: (498957862, 2147483647) gave masm 504025574
/// against native 195632602. It reproduced at every opt-level and every
/// `-C debuginfo` level (DWARF did NOT mask this shape) and was correct only
/// without `+wide-arithmetic`.
///
/// Fixed by the guest toolchain bump to nightly-2026-09-01 (fd6f5b171), not by
/// midenc; re-verified 2026-09-17 on the pinned rows and the random pairs.
/// Evidence that the wasm changed rather than the compiler: `wasmtime run -W
/// wide-arithmetic=y --invoke entrypoint differential_sat_add_u128.wasm
/// 498957862 2147483647` now returns 195632602, the native value, where it
/// used to return the MASM one — and the `i64.add128` is still in the wasm, so
/// the case still exercises the wide add.
///
/// Bounded by passing siblings: i128 `saturating_add`/`saturating_sub`
/// (sign-xor overflow test, `sat_i128`), u128 `checked_add` /
/// `overflowing_add` / `checked_sub` / `overflowing_sub` (add128_checked), and
/// u128 `checked_mul`/`saturating_mul` (`mul_wide_u` + `hi != 0`).
#[test]
fn sat_add_u128() {
    run_case("sat_add_u128", include_str!("../cases/case_sat_add_u128.rs"));
}

/// Regression guard for the `sat_add_u128` divergence, formerly the pinned
/// `#[ignore]`d twin: (1, 1) makes a = 2^96 + 2^64 + 1 and b = 2^96 + 2^32 (no
/// overflow; native folds the true sum 1539, the old wasm saturated to 3
/// because the stale high limb compared below a's), plus the all-ones row (a
/// genuine overflow) and a mixed row.
#[test]
fn sat_add_u128_repro() {
    run_case_with_inputs(
        "sat_add_u128_repro",
        include_str!("../cases/case_sat_add_u128.rs"),
        &[(1, 1), (0xffffffff, 0xffffffff), (0x12345678, 0x9abcdef0)],
    );
}

/// Formerly `#[ignore]`d (F9), the `i64.sub128` member of the family:
/// `u128::saturating_sub` on dynamic operands. Same toolchain and
/// `+wide-arithmetic` feature as sat_add_u128; the borrow test `diff > a` read
/// the difference's high limb through a `local.get` placed BEFORE the
/// `i64.sub128` that defines it (WAT: `local.get 4` ... `i64.sub128`
/// `local.set 4` `local.tee 5` ... `i64.gt_u` ... `select`), so
/// (2763801839, 2596936063) gave masm 1883194482 against native 246915218.
///
/// Fixed by the nightly-2026-09-01 guest toolchain; re-verified 2026-09-17
/// (wasmtime on the harness-built wasm now returns 246915218 for that pair and
/// 3758096832 for the pinned (u32::MAX, 2^31) row, both the native values, and
/// the `i64.sub128` is still in the wasm).
#[test]
fn sat_sub_u128() {
    run_case("sat_sub_u128", include_str!("../cases/case_sat_sub_u128.rs"));
}

/// Regression guard for the `sat_sub_u128` divergence, formerly the pinned
/// `#[ignore]`d twin: the (u32::MAX, 2^31) row (a > b, no borrow; the old wasm
/// saturated because the stale high limb compared above a's — masm 3758096384
/// against native 3758096832) plus a genuine-borrow row and a mixed row.
#[test]
fn sat_sub_u128_repro() {
    run_case_with_inputs(
        "sat_sub_u128_repro",
        include_str!("../cases/case_sat_sub_u128.rs"),
        &[(0xffffffff, 0x80000000), (0, 0xffffffff), (0x12345678, 0x9abcdef0)],
    );
}

/// Nearest passing neighbour of the formerly-ignored `sat_add_u128` /
/// `sat_sub_u128`
/// guest-LLVM miscompiles: i128 `saturating_add` / `saturating_sub` on
/// both-sign operands in straight-line, `#[inline(never)]` helper and loop
/// forms. The signed forms test overflow with a sign xor on the
/// `i64.add128` / `i64.sub128` high limb instead of the unsigned `sum < a`
/// limb compare, and LLVM stackifies them in a valid order (standalone
/// builds agree with native at every opt-level and debuginfo level).
#[test]
fn sat_i128() {
    run_case("sat_i128", include_str!("../cases/case_sat_i128.rs"));
}

/// Pinned edge grid for `sat_i128`: both high bits set (a and b near
/// i128::MIN, the sum saturates to MIN), both clear near MAX (the difference
/// of a negative and a positive saturates), all-ones and zero rows, and the
/// 2^32 +/- 1 low limbs.
#[test]
fn sat_i128_edges() {
    run_case_with_inputs(
        "sat_i128_edges",
        include_str!("../cases/case_sat_i128.rs"),
        &[
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0x7fffffff),
            (0x7fffffff, 0x80000000),
            (0x80000000, 0x7fffffff),
            (0xffffffff, 0xffffffff),
            (0, 0),
            (1, 1),
            (0xffffffff, 1),
            (1, 0xffffffff),
            (0x80000001, 0x7fffffff),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Formerly a guest-toolchain miscompile masked by DWARF, now a plain guard
/// (campaign 31): BOTH words of one wide result used as values
/// (`hi ^ lo.rotate_left(7)`) for each of `i64.mul_wide_u`, `mul_wide_s`,
/// `add128` and `sub128` in its own `#[inline(never)]` helper — the simplest
/// F9 shape. A standalone `rustc --target wasm32-wasip1 -C
/// target-feature=+wide-arithmetic` build reads the high word through a
/// `local.get` placed BEFORE the op that defines it (the helper starts with
/// `local.get N`, the op's `local.set N` follows) and disagrees with native
/// on 39/40 probe inputs at opt-level 1/2/3/s/z with `-C debuginfo=0` or `1`
/// (wasmtime 48 `-W wide-arithmetic=y` shows the wrong value); `-C
/// debuginfo=2` pinned the definitions and the same source was correct. That
/// was measured on nightly-2026-04-30, where this case passed ONLY because the
/// harness builds guests with `debug = 2`. The nightly-2026-09-01 toolchain
/// fixed the family, and this case now passes at `FUZZA_GUEST_DEBUG=0` too
/// (campaign 31 sweep, 2026-09-17), so the DWARF masking is no longer
/// load-bearing and the test is a plain wide-op guard.
#[test]
fn wide_words() {
    run_case("wide_words", include_str!("../cases/case_wide_words.rs"));
}

/// Pinned edge grid for `wide_words`: u64::MAX x u64::MAX / -1 x -1, 2^63
/// products, 2^32 +/- 1 operands, zero, and limb-swapped u128 pairs whose
/// sums carry and whose differences borrow across the 64-bit limb.
#[test]
fn wide_words_edges() {
    run_case_with_inputs(
        "wide_words_edges",
        include_str!("../cases/case_wide_words.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0x7fffffff),
            (1, 1),
            (0, 0),
            (1, 0xffffffff),
            (0xffffffff, 1),
            (0x80000000, 1),
            (1, 0x80000000),
            (0x80000001, 0x7fffffff),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Formerly a guest-toolchain miscompile masked by DWARF, now a plain guard
/// (campaign 31): loops whose wide result's high word is compared against a shift
/// of its low word (`hi != lo >> 60`) to decide a `break`, for
/// `i64.mul_wide_u`, `add128` and `sub128` (one `#[inline(never)]` helper
/// each; the `mul_wide_s` form of this compare is the formerly-ignored pow_i64 /
/// checked_mul_i64 family). Standalone builds with `+wide-arithmetic` read
/// the high word from the previous iteration's local (zero on the first
/// trip): the mul form failed at every opt-level, the add/sub forms at
/// opt-level 2 and 3, with `-C debuginfo=0` or `1`; `debuginfo=2` masked all
/// three, so it passed under the harness's `debug = 2` like wide_words. That
/// was nightly-2026-04-30; the nightly-2026-09-01 toolchain fixed the family
/// and this case passes at `FUZZA_GUEST_DEBUG=0` too (campaign 31 sweep,
/// 2026-09-17).
#[test]
fn wide_loop_cmp() {
    run_case("wide_loop_cmp", include_str!("../cases/case_wide_loop_cmp.rs"));
}

/// Pinned edge grid for `wide_loop_cmp`: trip counts 1..8 (input2 & 7 + 1)
/// on all-ones, sign-bit and 2^32 +/- 1 operands, so the break condition is
/// taken on the first trip, on a later trip, or never.
#[test]
fn wide_loop_cmp_edges() {
    run_case_with_inputs(
        "wide_loop_cmp_edges",
        include_str!("../cases/case_wide_loop_cmp.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000007),
            (0x7fffffff, 0x7fffffff),
            (1, 1),
            (0, 0),
            (0, 7),
            (1, 0xfffffff8),
            (0xffffffff, 1),
            (0x80000001, 0x7ffffffe),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Nearest passing neighbours of the `wide_words` / `wide_loop_cmp` F9
/// shapes: wide results whose high word alone (or low word alone) is used,
/// whose high word is compared against a constant (`hi != 0`, `hi < 0`) or
/// against the low word in a plain `if` (`hi < lo`), or which select a value
/// (`if hi != lo >> k { x } else { lo }`), for `mul_wide_u` / `mul_wide_s` /
/// `add128` / `sub128` in `#[inline(never)]` helpers. None of these places
/// the multi-result op inside the second operand subtree of a binary op
/// whose first operand is the high word, and standalone builds agree with
/// native at every opt-level and debuginfo level.
#[test]
fn mul_hi_only() {
    run_case("mul_hi_only", include_str!("../cases/case_mul_hi_only.rs"));
}

/// Pinned edge grid for `mul_hi_only`: MAX x MAX and -1 x -1 (high word all
/// ones / zero), 2^63 x 2^63, sign-bit rows for `hi < 0`, zero and 2^32 +/- 1
/// operands, and limb-swapped u128 pairs with carries and borrows.
#[test]
fn mul_hi_only_edges() {
    run_case_with_inputs(
        "mul_hi_only_edges",
        include_str!("../cases/case_mul_hi_only.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0x7fffffff),
            (0x7fffffff, 0xffffffff),
            (1, 1),
            (0, 0),
            (1, 0xffffffff),
            (0xffffffff, 1),
            (0x80000000, 1),
            (0x80000001, 0x7fffffff),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Nearest passing neighbours of the i64 F9 family (checked_mul_i64 /
/// sat_mul_i64 / pow_i64): the unsigned overflow-checked multiplies — u64
/// `checked_mul`, `overflowing_mul` (flag as a value and feeding a `break`),
/// `saturating_mul` and `checked_pow` — in `#[inline(never)]` helper and
/// loop forms (ovf_mul / int_logs cover the straight-line forms). Their
/// overflow test is `hi != 0` on the `i64.mul_wide_u` high word, a unary
/// `i64.eqz` with no second operand subtree for the multiply to sink into;
/// standalone builds agree with native at every opt-level and debuginfo
/// level.
#[test]
fn u64_sat_forms() {
    run_case("u64_sat_forms", include_str!("../cases/case_u64_sat_forms.rs"));
}

/// Pinned edge grid for `u64_sat_forms`: u64::MAX x u64::MAX (every form
/// overflows), 2^63 x 2, 2^32 +/- 1 operands, exponents 0..7 through
/// `input2 & 7`, zero, one, and mixed rows.
#[test]
fn u64_sat_forms_edges() {
    run_case_with_inputs(
        "u64_sat_forms_edges",
        include_str!("../cases/case_u64_sat_forms.rs"),
        &[
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0x80000000, 2),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0x7fffffff),
            (1, 1),
            (0, 0),
            (0, 7),
            (1, 0xffffffff),
            (0xffffffff, 1),
            (0x80000001, 0x7fffffff),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Formerly `#[ignore]`d (F9) at guest opt-level 1 only
/// (`--optimize=basic`): u128 `checked_add` accumulated in a loop. rustc
/// 1.97.0-nightly (c935696dd 2026-04-29, LLVM 22.1.4) at `-C opt-level=1`
/// emitted the carry test `sum < a` with the `local.get` of the sum's high
/// limb placed BEFORE the `i64.add128` that defines it (WAT: the loop body
/// opened `local.get 1 local.get 1 ... i64.add128 local.set 1 local.tee 9 ...
/// i64.lt_u local.get 1 ... i64.lt_u`), so every trip compared the previous
/// trip's high limb (zero on the first): (3, 3) gave masm 8826 against native
/// 11898 while the same shape passed at the default level (add128_checked).
///
/// Fixed by the nightly-2026-09-01 guest toolchain; re-verified 2026-09-17
/// (wasmtime on the harness-built `--optimize=basic` wasm returns 11898 for
/// (3, 3) and 512 for the pinned (1, 1) row, both the native values, and the
/// `i64.add128` is still there). Kept as the opt-level-1 guard of the wide
/// add in a loop.
#[test]
fn chk_add_u128_o1() {
    run_case_with_flags(
        "chk_add_u128_o1",
        include_str!("../cases/case_chk_add_u128_o1.rs"),
        &["--optimize=basic"],
    );
}

/// Regression guard for the `chk_add_u128_o1` divergence, formerly the pinned
/// `#[ignore]`d twin: the same case pinned to the (1, 1) and (3, 3) rows at
/// `--optimize=basic`. On (1, 1) the operands are a = 2^96 + 2^32 + 1 and
/// y | 1 = 2^96 + 2^64 + 2^32 + 1 over two trips — no overflow, but the old
/// wasm took the `None` arm on the first trip because the stale high limb read
/// 0 < a's (native 512 vs masm 1536).
#[test]
fn chk_add_u128_o1_repro() {
    run_case_with_flags_and_inputs(
        "chk_add_u128_o1_repro",
        include_str!("../cases/case_chk_add_u128_o1.rs"),
        &["--optimize=basic"],
        &[(1, 1), (3, 3)],
    );
}

/// Formerly `#[ignore]`d (F9), the `i64.mul_wide_u` member of the family in a
/// plain value idiom: the fixed-point multiply
/// `((a as u128 * b as u128) >> 32) as u64` in straight-line code. The
/// limb-straddling shift recombines `(hi << 32) | (lo >> 32)`, and rustc
/// 1.97.0-nightly (c935696dd 2026-04-29, LLVM 22.1.4) emitted the `local.get`
/// of the high word BEFORE the `i64.mul_wide_u` that defines it (WAT:
/// `local.get 2` ... `i64.mul_wide_u` `local.set 2` `i64.const 32`
/// `i64.shr_u` ... `i64.shl` `i64.or`), so (31, 2644960829) gave masm
/// 4029322267 against native 3877598572.
///
/// Fixed by the nightly-2026-09-01 guest toolchain; re-verified 2026-09-17
/// (wasmtime returns 3877598572 for that pair and 0 for the pinned (1, 1) row,
/// both the native values, and the `i64.mul_wide_u` is still in the wasm).
/// Bounded by the `>> 64` high-word-only form (mul_hi_only) and the
/// dynamic-count shift (`__lshrti3`, u128_shifts).
#[test]
fn fixmul_u64() {
    run_case("fixmul_u64", include_str!("../cases/case_fixmul_u64.rs"));
}

/// Regression guard for the `fixmul_u64` divergence, formerly the pinned
/// `#[ignore]`d twin: (1, 1) makes a = 2^32 and b = 2^32 + 1, product
/// 2^64 + 2^32 (hi 1, lo 2^32), so the true `>> 32` is 2^32 + 1 and the old
/// wasm's stale high word gave a different high half (native 0 vs masm 1);
/// plus the all-ones and a mixed row.
#[test]
fn fixmul_u64_repro() {
    run_case_with_inputs(
        "fixmul_u64_repro",
        include_str!("../cases/case_fixmul_u64.rs"),
        &[(1, 1), (0xffffffff, 0xffffffff), (0x12345678, 0x9abcdef0)],
    );
}

/// The passing sibling of [`parse_i64_hand`] with the wide path removed: the
/// same `str::parse::<i64>` of a runtime-length slice with the length capped
/// below sixteen, so LLVM emits only the plain `i64.mul` accumulation and no
/// constant is shared with a sign-extension. Agrees with native on every
/// pinned row.
#[test]
fn parse_i64_short() {
    run_case_with_inputs(
        "parse_i64_short",
        include_str!("../cases/case_parse_i64_short.rs"),
        &[(0, 0), (0, 1), (0, 3), (0, 14), (1, 7)],
    );
}

/// Formerly `#[ignore]`d (F18), root-caused 2026-09-10 and FIXED UPSTREAM by
/// ef358e356 (the coercion folders allocate a fresh immediate per result
/// instead of mutating the operand's); re-verified passing 2026-09-17 on the
/// pinned rows, with nightly-2026-09-01 AND nightly-2026-04-30 guests — the
/// toolchain-independence is what makes it a compiler fix rather than a shape
/// shift. Kept as the two-path regression guard. What it used to do:
///
/// the two accumulation paths `core` generates for a signed
/// 64-bit parse, written by hand in one function with no `core::str::parse` —
/// an overflow-checked path (`checked_mul(10)` + `checked_add`, which LLVM
/// lowers through `i64.mul_wide_s acc, 10`) for a sixteen-byte slice and a
/// plain wrapping path (`* 10 + digit`) for shorter ones. At `(0, 0)` the
/// slice is `"9"`: native returns 9, MASM returns 0.
///
/// MECHANISM: the frontend sign-extends both `mul_wide_s` operands to `i128`,
/// so the wasm's single `i64.const 10` becomes `%17 = arith.constant 10 :
/// i64` feeding both `arith.sext %17 : i128` (wide path) and the plain
/// `arith.mul %acc, %17` (plain path). `Sext::fold`
/// (dialects/arith/src/ops/coercions.rs, the in-place `fold`, unlike
/// `fold_with` which clones) obtains the constant's attribute through
/// `foldable_operand_of_trait` — a shared reference into the defining
/// `arith.constant` — and calls `set_from_immediate_lossy(Immediate::I128(10))`
/// on it, so after canonicalization `%17` carries an `i128` immediate under
/// its `i64` result type. `arith::Constant::emit` pushes from the immediate:
/// `push.0 push.0 push.0 push.10` (four felts, modelled as four) feed the
/// two-felt `intrinsics::i64::wrapping_mul`, the operand stack is misaligned
/// by two felts, and the digit add consumes the leftover zeros. The same
/// in-place mutation exists in `Zext::fold` and `Trunc::fold`.
/// BOUNDED by [`parse_i64_hand11`] (the plain path multiplies by 11 — no
/// shared constant — and every plain row is correct) and by
/// [`parse_i64_short`] (no wide path at all). The `core` producer is
/// `corelib::core_parse_i64`. An `arith.constant` verifier check that the
/// immediate's type matches the result type would have caught it.
#[test]
fn parse_i64_hand() {
    run_case_with_inputs(
        "parse_i64_hand",
        include_str!("../cases/case_parse_i64_hand.rs"),
        &[(0, 0), (0, 1), (0, 3), (0, 14), (0, 15), (1, 7)],
    );
}

/// The discriminating sibling of [`parse_i64_hand`]: identical except that
/// the plain path multiplies by 11, so the wide path's sign-extended constant
/// 10 is no longer shared with it — and every plain row agrees with native.
/// The sixteen-digit row `(0, 15)` used to be pinned OUT: it takes the
/// overflow-checked path and returned the overflow marker on MASM AND under
/// wasmtime (`-W wide-arithmetic=y`: -559038737) against native 2097153, i.e.
/// the F9 guest-toolchain miscompile of `checked_mul`. That row is back in
/// since the nightly-2026-09-01 bump fixed F9 (campaign 31), so the case now
/// covers BOTH parse paths.
#[test]
fn parse_i64_hand11() {
    run_case_with_inputs(
        "parse_i64_hand11",
        include_str!("../cases/case_parse_i64_hand11.rs"),
        &[(0, 0), (0, 1), (0, 3), (0, 14), (0, 15), (1, 7)],
    );
}

/// Formerly `#[ignore]`d: the minimal, loop-free reproducer of F18 (campaign
/// 28), FIXED UPSTREAM by ef358e356 (the coercion folders allocate a fresh
/// immediate per result); re-verified passing 2026-09-17 on the pinned rows
/// with nightly-2026-09-01 AND nightly-2026-04-30 guests, so the fix is
/// midenc's and not a guest-toolchain shape shift. Kept as the minimal
/// regression guard for shared coercion constants. What it used to do:
///
/// eight lines of
/// straight-line Rust in which the same `i64` constant 10 feeds a widening
/// multiply and a plain `i64` multiply of another value. `(a as i128) * 10`
/// lowers to `i64.mul_wide_s`, whose operands the wasm frontend sign-extends
/// (`arith.sext %c : i128`); `Sext::fold`
/// (dialects/arith/src/ops/coercions.rs) reads the constant's attribute
/// through `foldable_operand_of_trait` — a shared reference into the defining
/// `arith.constant` — and calls `set_from_immediate_lossy` on it, and the
/// folder's `try_get_or_create_constant` then materialises the `i128` constant
/// around that SAME attribute object. The `i64` constant the plain multiply
/// still uses therefore carries an `I128` immediate under an `i64` result
/// type, and `arith::Constant::emit` pushes from the immediate: the MASM
/// materialises that constant as `push.0 push.0 push.0 push.10` (four felts)
/// at BOTH use sites, including the one feeding
/// `exec.::intrinsics::i64::wrapping_mul`, which consumes two — the operand
/// stack is misaligned by two felts from there on. The printed HIR is no help
/// (it shows the result type, not the immediate variant): it prints
/// `arith.constant 10 : i64` either way.
/// GUEST-ARBITRATED: only the HIGH word of the wide product is consumed (the
/// `mul_hi_only` shape), and `wasmtime 48 -W wide-arithmetic=y` on the
/// harness-built wasm returns the native answer on the pinned rows (30 at
/// (1, 2), 44 at (3, 5)), so this is midenc's defect and not the F9
/// guest-toolchain family. The wrong-width push is present in the MASM at all
/// four optimization levels; the value check here runs at the default one.
/// BOUNDED by [`sext_const_split`] (the plain multiply uses 11, so nothing is
/// shared — same shape, correct answer), by [`zext_const_shared`] (the
/// unsigned twin passes) and by [`trunc_const_shared`] (the truncating twin
/// passes). The `core` producer of the same defect is `corelib::core_parse_i64`
/// and the hand-written two-path form is [`parse_i64_hand`].
#[test]
fn sext_const_shared() {
    run_case_with_inputs(
        "sext_const_shared",
        include_str!("../cases/case_sext_const_shared.rs"),
        &[(1, 2), (3, 5), (0, 0), (0xffff_ffff, 1), (7, 0xffff_ffff)],
    );
}

/// The discriminating sibling of [`sext_const_shared`]: identical except that
/// the plain multiply uses 11, so the sign-extended constant is not shared
/// with a two-felt `i64` use. Agrees with native on the same pinned rows.
#[test]
fn sext_const_split() {
    run_case_with_inputs(
        "sext_const_split",
        include_str!("../cases/case_sext_const_split.rs"),
        &[(1, 2), (3, 5), (0, 0), (0xffff_ffff, 1), (7, 0xffff_ffff)],
    );
}

/// F18 reach guard, UNSIGNED folder (campaign 28): the [`sext_const_shared`]
/// shape with `u64`/`u128`, i.e. a `u64` constant shared between an
/// `i64.mul_wide_u` and a plain `u64` multiply. It PASSES, and the MASM says
/// why: the `I64MulWideU` translation bitcasts each operand to `u64` before
/// zero-extending it (`hir.bitcast` then `arith.zext %x : u128`, frontend
/// mod.rs), so the attribute `Zext::fold` mutates belongs to the `u64`
/// constant materialised for the bitcast, not to the `i64` constant the plain
/// multiply uses — the plain multiply is fed `push.0 push.10` (two felts,
/// correct) while the wide one gets four. `Sext` has no such bitcast:
/// `I64MulWideS` sign-extends the wasm operand directly. This test fails if a
/// fix (or a regression) makes the unsigned path alias the shared constant.
#[test]
fn zext_const_shared() {
    run_case_with_inputs(
        "zext_const_shared",
        include_str!("../cases/case_zext_const_shared.rs"),
        &[(1, 2), (3, 5), (0, 0), (0xffff_ffff, 1), (7, 0xffff_ffff)],
    );
}

/// F18 reach guard, TRUNCATING folder (campaign 28): a 64-bit rotate count
/// constant — every 64-bit shift/rotate count goes through
/// `mask_movement_count`'s `builder.trunc(count, U32)`, so this folder runs on
/// most of the corpus — shared with a plain `i64` use of the same constant.
/// It PASSES: the MASM pushes the count as one felt (`push.63 push.24 u32and`,
/// the folded count band) and the `i64` addend as two (`push.0 push.24`), so
/// the two constants are distinct attributes here. The same holds when the two
/// uses sit in different branches and when the plain use is a multiply
/// (campaign-28 probes). This test fails if the truncating folder starts
/// aliasing shared constants the way `Sext::fold` does.
#[test]
fn trunc_const_shared() {
    run_case_with_inputs(
        "trunc_const_shared",
        include_str!("../cases/case_trunc_const_shared.rs"),
        &[(1, 2), (3, 5), (0, 0), (0xffff_ffff, 1), (7, 0xffff_ffff)],
    );
}

/// F18 LADDER, one rung past the campaign-28 boundary (campaign 31,
/// 2026-09-17). Campaign 28 measured F18's reach as "only the SIGNED folder
/// has a plain-Rust producer"; with ef358e356 the folders no longer mutate
/// their operand's attribute, so this case makes ALL THREE compete for the
/// same literal 10 in one function — `Sext::fold` through an `i128` widening
/// multiply, `Zext::fold` through a `u128` one, `Trunc::fold` through a
/// 64-bit rotate count — and then consumes the literal again through four
/// plain uses at three widths, with BOTH words of each wide product used so a
/// wrong-width push cannot hide in a dropped limb. Native-grid-checked over
/// the 1225 boundary pairs before it was kept.
#[test]
fn coerce_const_fanout() {
    run_case_with_inputs(
        "coerce_const_fanout",
        include_str!("../cases/case_coerce_const_fanout.rs"),
        &[(1, 2), (3, 5), (0, 0), (0xffff_ffff, 1), (7, 0xffff_ffff), (10, 10)],
    );
}

/// F9 LADDER, one rung past the old boundary (campaign 31, 2026-09-17). Every
/// F9 reproducer passes with nightly-2026-09-01 guests, so this case pushes
/// the shape as far as plain Rust reaches: a loop chaining `i64.add128`,
/// `i64.sub128`, `i64.mul_wide_u` and `i64.mul_wide_s` that consumes BOTH
/// limbs of every result on every trip, feeds each back into the next
/// operation and into the loop's exit test, and keeps the overflow/borrow
/// predicates live through the saturating and checked forms — the union of
/// every shape the family used to break on. Native-grid-checked over the 1225
/// boundary pairs before it was kept.
#[test]
fn wide_limbs_chain() {
    run_case("wide_limbs_chain", include_str!("../cases/case_wide_limbs_chain.rs"));
}

/// Pinned grid for [`wide_limbs_chain`]: the trip counts at both ends of
/// `input1 % 7 + 2`, the limb boundaries, and the equal pair.
#[test]
fn wide_limbs_chain_edges() {
    run_case_with_inputs(
        "wide_limbs_chain_edges",
        include_str!("../cases/case_wide_limbs_chain.rs"),
        &[
            (0, 0),
            (0xffff_ffff, 0xffff_ffff),
            (6, 0xffff_ffff),
            (1, 0x8000_0000),
            (0x7fff_ffff, 1),
            (13, 13),
        ],
    );
}

/// F18 LADDER rung 2 (campaign 31): the [`coerce_const_fanout`] fan-out
/// repeated for TEN distinct literals inside a bottom-tested loop, so every
/// literal is a count band crossing the loop AND is shared between
/// `Sext::fold` (an `i128` widening multiply), `Trunc::fold` (a rotate count)
/// and a plain `i64` use. It was written to find the class that bounds the
/// coercion ladder and did NOT find one: it compiles and matches native at all
/// four optimization levels and without guest DWARF, so ten shared coercion
/// constants across a loop are not enough pressure on their own — the freight
/// that does reach a boundary is live 64-bit VALUES, not shared counts
/// ([`wide_limbs_freight_oz`]).
#[test]
fn coerce_const_bands() {
    run_case("coerce_const_bands", include_str!("../cases/case_coerce_const_bands.rs"));
}

/// F9 LADDER rung 2 (campaign 31): the [`wide_limbs_chain`] shape with FOUR
/// u128 accumulators live across the loop, all consumed limb-by-limb in one
/// joining expression per trip, plus three crossing rotate bands — eight u64
/// limbs of freight, the range campaign 20/21 measured the spill cliff on.
/// This is the rung where the wide-arithmetic ladder stops being about wide
/// arithmetic: it compiles and matches native at the default level, at
/// `--optimize=max`, at `--optimize=basic` and without guest DWARF, and stops
/// at `--optimize=size-min` in the F6 spill cluster ([`wide_limbs_freight_oz`]).
/// So the class that now bounds wide-arithmetic freight is a spill-placement
/// class, not a wide-arithmetic one.
#[test]
fn wide_limbs_freight() {
    run_case("wide_limbs_freight", include_str!("../cases/case_wide_limbs_freight.rs"));
}

/// COMPILE-TIME COMPILER PANIC, [`wide_limbs_freight`] at
/// `--optimize=size-min` (campaign 31, 2026-09-17): `invalid operand stack
/// index (10): requires access to more than 16 elements` at
/// codegen/masm/src/emit/mod.rs:623. CLASSIFIED F6 by trace, not F17: the
/// spills trace (`MIDENC_TRACE='analysis:spills=trace,pass:spills=trace'`)
/// shows `edges to split = 1` for `entrypoint` followed by `erase unused
/// reload %55`, and the drop trace
/// (`MIDENC_TRACE='codegen:operand-scheduling=trace'`) ends on `hir.store_local
/// %55` — the erased reload and the failing spill store are the SAME value, so
/// the stale-dominator-tree erasure is what leaves it stranded outside the
/// window rather than an independent placement miss. The other three
/// optimization levels and the no-DWARF build compile the same source.
/// Compile-time — no inputs involved. Un-ignore with the other F6
/// reproducers.
#[test]
#[ignore = "compiler panic at --optimize=size-min: 'invalid operand stack index (10): requires \
            access to more than 16 elements' at codegen/masm/src/emit/mod.rs:623 — F6 (edges to \
            split = 1, the erased reload %55 is the operand of the failing spill store); \
            compile-time, no inputs involved"]
fn wide_limbs_freight_oz() {
    run_case_with_flags(
        "wide_limbs_freight_oz",
        include_str!("../cases/case_wide_limbs_freight.rs"),
        &["--optimize=size-min"],
    );
}

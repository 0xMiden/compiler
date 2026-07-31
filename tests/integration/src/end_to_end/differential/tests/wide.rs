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

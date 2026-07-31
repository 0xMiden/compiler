//! Boundary-value runtime semantics asserted via pinned input grids.

use super::super::harness::{run_case, run_case_with_inputs};

/// Dynamic-count logical shifts and rotates on u32/u64 (wrapping_shl/shr,
/// rotate_left/right) — asserts the VM masks the count (`% width`) exactly
/// like Rust.
#[test]
fn shift_counts() {
    run_case("shift_counts", include_str!("../cases/case_shift_counts.rs"));
}

/// Pinned edge grid for `shift_counts`: counts 0, 1, width-1, width, width+1,
/// 2*width, and over-2*width (67/96/131) on values 0/1/0x7FFFFFFF/0x80000001/
/// u32::MAX — boundary count pairs proptest essentially never draws.
#[test]
fn shift_counts_edges() {
    run_case_with_inputs(
        "shift_counts_edges",
        include_str!("../cases/case_shift_counts.rs"),
        &[
            (0x80000001, 0),
            (0x80000001, 1),
            (0x80000001, 31),
            (0x80000001, 32),
            (0x80000001, 33),
            (0x80000001, 63),
            (0x80000001, 64),
            (0x80000001, 67),
            (0x80000001, 131),
            (1, 63),
            (0xffffffff, 32),
            (0x7fffffff, 31),
            (0, 64),
            (0xdeadbeef, 96),
        ],
    );
}

/// Arithmetic shift right on negative values with dynamic unmasked counts —
/// the edge arms of `::intrinsics::i32/i64::checked_shr` plus the constant
/// `>> 31` / `>> 63` sign-mask idiom.
#[test]
fn ashr_neg() {
    run_case("ashr_neg", include_str!("../cases/case_ashr_neg.rs"));
}

/// Pinned edge grid for `ashr_neg`: MIN >> 0 == MIN, MIN >> width-1 == -1,
/// count == width masks to 0, over-width counts mask (67 -> 3), -1 >> c == -1;
/// row (0x80000000, 0) makes the i64 operand exactly i64::MIN.
#[test]
fn ashr_neg_edges() {
    run_case_with_inputs(
        "ashr_neg_edges",
        include_str!("../cases/case_ashr_neg.rs"),
        &[
            (0x80000000, 0),
            (0x80000000, 31),
            (0x80000000, 32),
            (0x80000000, 63),
            (0x80000000, 64),
            (0x80000000, 67),
            (0xffffffff, 0),
            (0xffffffff, 1),
            (0x7fffffff, 31),
            (0x7fffffff, 63),
            (0, 63),
            (1, 31),
        ],
    );
}

/// Unsigned u32/u64 division/remainder with `| 1`-guarded dynamic divisors —
/// the boundary relations (divisor 1 / equal / greater, dividend 0, high-bit
/// divisors) of `u32div`/`u32mod` and miden-core-lib `u64::div`/`u64::mod`.
#[test]
fn udiv_bounds() {
    run_case("udiv_bounds", include_str!("../cases/case_udiv_bounds.rs"));
}

/// Pinned edge grid for `udiv_bounds`: divisor 1 ((0,0), (MAX,1)), divisor ==
/// dividend ((5,5), (MAX,MAX)), divisor > dividend ((3,7), (1,MAX)), dividend
/// 0, and u64 divisors with the high bit set (largest-divisor path).
#[test]
fn udiv_bounds_edges() {
    run_case_with_inputs(
        "udiv_bounds_edges",
        include_str!("../cases/case_udiv_bounds.rs"),
        &[
            (0, 1),
            (5, 5),
            (3, 7),
            (0xffffffff, 1),
            (1, 0xffffffff),
            (0, 0),
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (0, 0x80000000),
            (0x80000000, 0),
            (7, 2),
            (2, 4),
        ],
    );
}

/// Signed i32 `/`+`%` and i64 `/` at the MIN/magnitude-1 boundaries, MIN/-1
/// unconstructible by design (odd numerators over negative divisors) — the
/// sign-fixup edges of `::intrinsics::{i32,i64}::checked_div`/`wrapping_mod`.
#[test]
fn sdiv_bounds() {
    run_case("sdiv_bounds", include_str!("../cases/case_sdiv_bounds.rs"));
}

/// Pinned edge grid for `sdiv_bounds`: row (0x80000000, 0) forces i32 MIN/1,
/// (MIN|1)/-1 == MAX, i64::MIN/1, and (i64::MIN|1)/-1 == i64::MAX all at
/// once; rows (0x00008000, 0) / (0x00800000, 0) pin the rotated remainder
/// numerators to MIN % 1 and (MIN|1) % -1; other rows pin -1/1, 1/-1, 0/1.
#[test]
fn sdiv_bounds_edges() {
    run_case_with_inputs(
        "sdiv_bounds_edges",
        include_str!("../cases/case_sdiv_bounds.rs"),
        &[
            (0x80000000, 0),
            (0x00008000, 0),
            (0x00800000, 0),
            (0x80000001, 0),
            (0xffffffff, 0),
            (0x7fffffff, 0),
            (1, 0),
            (0, 0),
            (0x80000000, 999),
            (0x80000000, 0x00010000),
            (0xdeadbeef, 123456),
            (12345, 4),
        ],
    );
}

/// Wrapping arithmetic at MIN/MAX: wrapping_neg/abs(MIN), MAX+1, MAX*MAX,
/// MIN sign-extending casts, u32::MAX widening to u64, and checked_add/sub/neg
/// None arms (LLVM legalizes checked ops to wrapping + compare).
#[test]
fn wrap_minmax() {
    run_case("wrap_minmax", include_str!("../cases/case_wrap_minmax.rs"));
}

/// Pinned edge grid for `wrap_minmax`: i32/i64 MIN rows (0x80000000, 0),
/// u32 MAX+1 and MAX*MAX rows, i32 MAX+1 -> MIN, i64 MAX+1 -> MIN
/// ((0x7FFFFFFF, 0xFFFFFFFF)), and the checked-op overflow/underflow rows.
#[test]
fn wrap_minmax_edges() {
    run_case_with_inputs(
        "wrap_minmax_edges",
        include_str!("../cases/case_wrap_minmax.rs"),
        &[
            (0x80000000, 0),
            (0x80000000, 1),
            (0xffffffff, 1),
            (0xffffffff, 0xffffffff),
            (0x7fffffff, 1),
            (0x7fffffff, 0xffffffff),
            (0, 0),
            (0, 1),
            (1, 0xffffffff),
            (0xaaaaaaaa, 0x55555555),
        ],
    );
}

/// leading_zeros/trailing_zeros/count_ones of exactly 0 and MAX on u32, u64,
/// and u128 — the clz(0) == width / ctz(0) == width saturation arms of the
/// bit-count intrinsics, never differentially asserted before.
#[test]
fn bitcnt_zero() {
    run_case("bitcnt_zero", include_str!("../cases/case_bitcnt_zero.rs"));
}

/// Pinned edge grid for `bitcnt_zero`: the all-zero row (0, 0) (clz/ctz
/// saturate at 32/64/128), the all-ones row, and limb-boundary single-bit
/// rows — ctz(1<<32) == 32 via (1, 0), clz == 32 via (0, 0x80000000).
#[test]
fn bitcnt_zero_edges() {
    run_case_with_inputs(
        "bitcnt_zero_edges",
        include_str!("../cases/case_bitcnt_zero.rs"),
        &[
            (0, 0),
            (0xffffffff, 0xffffffff),
            (0, 1),
            (1, 0),
            (0x80000000, 0),
            (0, 0x80000000),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x00010000, 0),
            (0xffff0000, 0x0000ffff),
        ],
    );
}

/// Zero- and boundary-length memory ops with disjoint ranges: element copies
/// of length 0/1, byte copies of length 0..=5 across the memcpy `% 4`
/// fastpath boundary, and a byte fill of length 0 — with fixed-index reads
/// asserting length-0 ops wrote nothing.
#[test]
fn memlen_zero() {
    run_case("memlen_zero", include_str!("../cases/case_memlen_zero.rs"));
}

/// Pinned edge grid for `memlen_zero`: (0,0) makes every copy/fill length 0;
/// (1,1) exactly 1; (4,4) puts the byte copy exactly on the element-fastpath
/// boundary (count 4); 2/3/5 pin the byte-tail fallback loop lengths.
#[test]
fn memlen_zero_edges() {
    run_case_with_inputs(
        "memlen_zero_edges",
        include_str!("../cases/case_memlen_zero.rs"),
        &[
            (0, 0),
            (1, 1),
            (4, 4),
            (5, 2),
            (3, 3),
            (2, 0),
            (0xffffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (7, 9),
            (12, 10),
        ],
    );
}

/// DELIBERATE PROBE: zero-length copy at an identical src == dst position
/// (opaquely-zero length and dst offset survive to a runtime memory.copy) —
/// length-0 ranges cannot overlap, so any VM-side abort would be a real
/// divergence in the memcopy_elements overlap assert.
#[test]
fn memnoop_same() {
    run_case("memnoop_same", include_str!("../cases/case_memnoop_same.rs"));
}

/// Pinned edge grid for `memnoop_same`: positions p = 0..=3 (including via
/// u32::MAX % 4) for the len-0 src == dst copy; every row must be a no-op on
/// both sides.
#[test]
fn memnoop_same_edges() {
    run_case_with_inputs(
        "memnoop_same_edges",
        include_str!("../cases/case_memnoop_same.rs"),
        &[
            (0, 0),
            (1, 1),
            (2, 3),
            (3, 2),
            (0xffffffff, 0),
            (0x80000004, 4),
            (7, 11),
            (123456789, 987654321),
        ],
    );
}

/// While-loops with `% 97`-derived trip counts and loop-carried u32/u64
/// values — the zero-trip (guard skips the rotated body) and one-trip edge
/// behavior of lifted scf.while regions at runtime.
#[test]
fn trip_loops() {
    run_case("trip_loops", include_str!("../cases/case_trip_loops.rs"));
}

/// Pinned edge grid for `trip_loops`: trip counts exactly 0 and 1 for both
/// loops in all combinations, including the modulus wrap rows 97 -> 0 and
/// 98 -> 1, plus a full-range row.
#[test]
fn trip_loops_edges() {
    run_case_with_inputs(
        "trip_loops_edges",
        include_str!("../cases/case_trip_loops.rs"),
        &[
            (0, 0),
            (1, 1),
            (0, 1),
            (1, 0),
            (97, 97),
            (98, 98),
            (2, 1),
            (96, 2),
            (0x80000000, 1),
            (0xffffffff, 0xffffffff),
        ],
    );
}

/// i8/i16 sign boundaries (0x7F/0x80/0xFF, 0x7FFF/0x8000/0xFFFF) through
/// sign-extending table loads (i32/i64.load8_s/16_s) and `as i8/i16 as
/// i32/i64` truncate-sign-extend chains.
#[test]
fn subword_sign() {
    run_case("subword_sign", include_str!("../cases/case_subword_sign.rs"));
}

/// Pinned edge grid for `subword_sign`: exact boundary bytes/halfwords into
/// the extend chains (0x7F/0x80/0xFF, 0x7FFF/0x8000/0xFFFF), table indexes
/// hitting the MIN/MAX/-1 entries, and truncation-before-sext rows
/// (0x100 -> 0, 0xFF80 -> -128, 0xFFFF8000 -> -32768).
#[test]
fn subword_sign_edges() {
    run_case_with_inputs(
        "subword_sign_edges",
        include_str!("../cases/case_subword_sign.rs"),
        &[
            (0x7f, 0x7fff),
            (0x80, 0x8000),
            (0xff, 0xffff),
            (0, 1),
            (1, 0),
            (2, 2),
            (0x100, 0x10000),
            (0x17f, 0x17fff),
            (0xff80, 0xffff8000),
            (5, 3),
        ],
    );
}

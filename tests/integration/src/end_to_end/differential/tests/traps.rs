//! Trap parity: cases that deliberately panic on some inputs, compared with
//! `run_case_traps` so that both targets must trap on exactly the same
//! inputs and agree on the value everywhere else.
//!
//! Every case comes with an `_edges` twin that pins the boundary grid — the
//! last valid index, the first invalid one, `mid == len`, divisor zero,
//! `MIN / -1`, the shift count at the type's width — because random draws
//! reach those rows rarely or never. The grid is what makes the oracle
//! sharp; the fuzzed base case only adds bulk.
//!
//! Guests are release builds with overflow checks OFF and
//! `-Cpanic=immediate-abort`, so `a + 1` at `u32::MAX` wraps silently while
//! every *language-mandated* panic (bounds checks, slice ranges, division by
//! zero, `MIN / -1`, `unwrap`, `assert!`) becomes a wasm `unreachable` that
//! the compiler lowers to a failing assertion.

use super::super::harness::{run_case_traps, run_case_traps_with_inputs};

/// Runtime index into a fixed `[u32; 8]` with indices reaching 0..12: the
/// out-of-range third must trap on both targets (`index out of bounds`), the
/// rest must agree on the element.
#[test]
fn trap_index() {
    run_case_traps("trap_index", include_str!("../cases/case_trap_index.rs"));
}

/// Pinned boundary rows for `trap_index`: the last valid index (7), the first
/// invalid one (8), the largest (11), index 0, and a wrapped valid index
/// (19 % 12 = 7), each with a distinct `input2`.
#[test]
fn trap_index_edges() {
    run_case_traps_with_inputs(
        "trap_index_edges",
        include_str!("../cases/case_trap_index.rs"),
        &[(7, 1), (8, 1), (11, 0xffff_ffff), (0, 0), (19, 2)],
    );
}

/// Two independent bounds checks in one function — a `[u8; 13]` indexed by
/// `input1 % 16` and a `[u32; 5]` indexed by `(input1 ^ input2) % 7` — so a
/// trap can come from either, including from an index that needs both
/// inputs.
#[test]
fn trap_bounds_u8() {
    run_case_traps("trap_bounds_u8", include_str!("../cases/case_trap_bounds_u8.rs"));
}

/// Pinned rows for `trap_bounds_u8`: the byte array's last valid index (12)
/// and first invalid one (13), the word array's last valid (j = 4) and first
/// invalid (j = 5), the origin, `u32::MAX` on both inputs (i = 15, out of
/// range), and a valid pair whose second index is computed from both inputs.
#[test]
fn trap_bounds_u8_edges() {
    run_case_traps_with_inputs(
        "trap_bounds_u8_edges",
        include_str!("../cases/case_trap_bounds_u8.rs"),
        &[(12, 12), (13, 13), (0, 4), (0, 5), (0, 0), (0xffff_ffff, 0xffff_ffff), (12, 16)],
    );
}

/// Nested `[[u16; 4]; 4]` indexing with an out-of-range value reachable in
/// each dimension independently, so the outer and the inner bounds check are
/// both exercised as the sole trap source.
#[test]
fn trap_bounds_nested() {
    run_case_traps("trap_bounds_nested", include_str!("../cases/case_trap_bounds_nested.rs"));
}

/// Pinned rows for `trap_bounds_nested`: the last valid cell (3, 3), the
/// first invalid row (4, 0), the first invalid column (0, 4), both invalid
/// (4, 4), the origin, and a wrapped valid pair (8 % 5 = 3).
#[test]
fn trap_bounds_nested_edges() {
    run_case_traps_with_inputs(
        "trap_bounds_nested_edges",
        include_str!("../cases/case_trap_bounds_nested.rs"),
        &[(3, 3), (4, 0), (0, 4), (4, 4), (0, 0), (8, 8)],
    );
}

/// `get(i).unwrap()` and `iter().nth(j).unwrap()` against a `static` table:
/// the same out-of-range index as `table[i]`, but panicking on `None`
/// instead of through a bounds check, and reading a data segment rather than
/// a stack array.
#[test]
fn trap_bounds_get() {
    run_case_traps("trap_bounds_get", include_str!("../cases/case_trap_bounds_get.rs"));
}

/// Pinned rows for `trap_bounds_get`: the last valid index (8) for both
/// accessors, the first invalid one for `get` (9, 0) and for `nth` (0, 9),
/// both invalid (11, 11), and two valid rows.
#[test]
fn trap_bounds_get_edges() {
    run_case_traps_with_inputs(
        "trap_bounds_get_edges",
        include_str!("../cases/case_trap_bounds_get.rs"),
        &[(8, 8), (9, 0), (0, 9), (11, 11), (0, 0), (8, 0)],
    );
}

/// An index that walks off the end of a `[u32; 12]` part-way through a loop,
/// so the trap sits behind several trips of real work rather than at the
/// function entry.
#[test]
fn trap_bounds_loop() {
    run_case_traps("trap_bounds_loop", include_str!("../cases/case_trap_bounds_loop.rs"));
}

/// Pinned rows for `trap_bounds_loop`: the largest base that survives all
/// five trips (7), the first that traps and does so on the LAST trip (8), a
/// base that traps on trip 1 (11), the origin, a base that traps on trip 0
/// (19), and a base whose `input2` is `u32::MAX`.
#[test]
fn trap_bounds_loop_edges() {
    run_case_traps_with_inputs(
        "trap_bounds_loop_edges",
        include_str!("../cases/case_trap_bounds_loop.rs"),
        &[(7, 1), (8, 1), (11, 0), (0, 0), (19, 5), (2, 0xffff_ffff)],
    );
}

/// Slice range indexing over a `[u32; 10]` with both endpoints in 0..12:
/// `&data[a..b]` panics both for `a > b` and for `b > len`, and the
/// open-ended `&data[a..]` / `&data[..b]` forms share the second.
#[test]
fn trap_slice_range() {
    run_case_traps("trap_slice_range", include_str!("../cases/case_trap_slice_range.rs"));
}

/// Pinned rows for `trap_slice_range`: the full slice (0, 10), the first
/// end-out-of-range (0, 11), a reversed range (5, 4), the empty slice at
/// `a == b == len` (10, 10), both endpoints out of range (11, 11), an
/// interior range, and a start past the end (12, 0).
#[test]
fn trap_slice_range_edges() {
    run_case_traps_with_inputs(
        "trap_slice_range_edges",
        include_str!("../cases/case_trap_slice_range.rs"),
        &[(0, 10), (0, 11), (5, 4), (10, 10), (11, 11), (3, 7), (12, 0)],
    );
}

/// `split_at` and `split_at_mut` on a `[u32; 8]` with the split point in
/// 0..11, driven by a different input each, so `mid > len` is reachable for
/// the shared and the mutable split independently.
#[test]
fn trap_slice_split() {
    run_case_traps("trap_slice_split", include_str!("../cases/case_trap_slice_split.rs"));
}

/// Pinned rows for `trap_slice_split`: `mid == len` on both splits (8, 8),
/// the first invalid shared split (9, 0), the first invalid mutable split
/// (0, 9), both invalid (10, 10), both empty-left (0, 0), a balanced split,
/// and a split that leaves exactly one element on the right.
#[test]
fn trap_slice_split_edges() {
    run_case_traps_with_inputs(
        "trap_slice_split_edges",
        include_str!("../cases/case_trap_slice_split.rs"),
        &[(8, 8), (9, 0), (0, 9), (10, 10), (0, 0), (4, 4), (7, 7)],
    );
}

/// `swap(i, 5)` out of bounds and `copy_from_slice` with a length mismatch:
/// the two element-moving slice methods that panic on their arguments rather
/// than on an index expression.
#[test]
fn trap_slice_copy() {
    run_case_traps("trap_slice_copy", include_str!("../cases/case_trap_slice_copy.rs"));
}

/// Pinned rows for `trap_slice_copy`: the last valid swap index (5) and the
/// first invalid one (6), a zero-length destination against a length-1
/// source (0, 0), matching lengths (0, 1), the largest swap index (8), a
/// 4-vs-5 length mismatch (0, 4), and a matching interior length (0, 3).
#[test]
fn trap_slice_copy_edges() {
    run_case_traps_with_inputs(
        "trap_slice_copy_edges",
        include_str!("../cases/case_trap_slice_copy.rs"),
        &[(5, 1), (6, 1), (0, 0), (0, 1), (8, 1), (0, 4), (0, 3)],
    );
}

/// `chunks_exact(k)` / `windows(k)` at `k == 0` and `rotate_left(r)` at
/// `r > len`: slice APIs whose *argument* is what panics, including the
/// legal `r == len` rotation that must not trap.
#[test]
fn trap_slice_chunks() {
    run_case_traps("trap_slice_chunks", include_str!("../cases/case_trap_slice_chunks.rs"));
}

/// Pinned rows for `trap_slice_chunks`: the smallest legal chunk/window size
/// (1, 0), the zero size on both iterators (0, 0) and (4, 1), the legal
/// full-length rotation (1, 8), the first illegal rotation (1, 9), the
/// largest legal chunk size (3, 3), and a rotation two past the end.
#[test]
fn trap_slice_chunks_edges() {
    run_case_traps_with_inputs(
        "trap_slice_chunks_edges",
        include_str!("../cases/case_trap_slice_chunks.rs"),
        &[(1, 0), (0, 0), (4, 1), (1, 8), (1, 9), (3, 3), (2, 10)],
    );
}

/// `u32` and `i32` division by runtime divisors: zero divisors and
/// `i32::MIN / -1` must trap on both targets, everything else must agree.
#[test]
fn trap_div_zero() {
    run_case_traps("trap_div_zero", include_str!("../cases/case_trap_div_zero.rs"));
}

/// Pinned rows for `trap_div_zero`: a plain quotient, `input2 % 4 == 0`
/// (unsigned divide by zero), `input2 == 0` (both divisions), `i32::MIN / -1`
/// (signed overflow), `i32::MIN / -2` (no overflow) and division by 1.
#[test]
fn trap_div_zero_edges() {
    run_case_traps_with_inputs(
        "trap_div_zero_edges",
        include_str!("../cases/case_trap_div_zero.rs"),
        &[
            (5, 3),
            (5, 4),
            (5, 0),
            (0x8000_0000, 0xffff_ffff),
            (0x8000_0000, 0xffff_fffe),
            (7, 1),
        ],
    );
}

/// 64-bit `/` and `%`, signed and unsigned — the widths that go through the
/// `u64`/`i64` division intrinsics on Miden rather than a native opcode —
/// with the zero divisor and `i64::MIN / -1` both reachable.
#[test]
fn trap_div_wide() {
    run_case_traps("trap_div_wide", include_str!("../cases/case_trap_div_wide.rs"));
}

/// Pinned rows for `trap_div_wide`, the classic signed-division grid built
/// from `input1 = i64::MIN`'s high word and the residue pair `(input2 % 4,
/// input2 % 5)`: `MIN / -1` (16), `MIN+1 / -1` (1), `MAX / -1` (6),
/// `0 / -1` (16 with a zero dividend), `MIN / 1` (8), an unsigned zero
/// divisor (5), a signed zero divisor (2), and an ordinary quotient (6).
#[test]
fn trap_div_wide_edges() {
    run_case_traps_with_inputs(
        "trap_div_wide_edges",
        include_str!("../cases/case_trap_div_wide.rs"),
        &[
            (0x8000_0000, 16),
            (0x8000_0000, 1),
            (0x7fff_ffff, 6),
            (0, 16),
            (5, 5),
            (5, 2),
            (0x8000_0000, 8),
            (7, 6),
        ],
    );
}

/// The `i32` division family around its two panicking edges: `div_euclid`,
/// `rem_euclid` and `checked_div(..).unwrap()` must trap on a zero divisor
/// and on `MIN / -1`, while `wrapping_div`, `overflowing_div` and
/// `checked_rem(..).unwrap_or` are defined there and must not.
#[test]
fn trap_div_euclid() {
    run_case_traps("trap_div_euclid", include_str!("../cases/case_trap_div_euclid.rs"));
}

/// Pinned rows for `trap_div_euclid`, the `(x, 0)` / `(MIN, -1)` grid:
/// divisor -2 (5, 0), divisor 0 (5, 2), `MIN / -1` (0x8000_0000, 1),
/// `MIN / -2` (0x8000_0000, 0), `MIN / 1` (0x8000_0000, 3), `MIN+1 / -1`,
/// `0 / -1`, and `MAX / -1` — only the zero divisor and `MIN / -1` trap.
#[test]
fn trap_div_euclid_edges() {
    run_case_traps_with_inputs(
        "trap_div_euclid_edges",
        include_str!("../cases/case_trap_div_euclid.rs"),
        &[
            (5, 0),
            (5, 2),
            (0x8000_0000, 1),
            (0x8000_0000, 0),
            (0x8000_0000, 3),
            (0x8000_0001, 1),
            (0, 1),
            (0x7fff_ffff, 1),
        ],
    );
}

/// A divisor that decrements to zero on a late loop trip, so the divide-by-
/// zero trap happens only after earlier trips have already divided
/// successfully.
#[test]
fn trap_div_loop() {
    run_case_traps("trap_div_loop", include_str!("../cases/case_trap_div_loop.rs"));
}

/// Pinned rows for `trap_div_loop`: the smallest start that survives all
/// four trips (4), a start that reaches zero on the last trip (3), zero from
/// the first trip (0), the largest start (9), and two wrapped starts (10, 13).
#[test]
fn trap_div_loop_edges() {
    run_case_traps_with_inputs(
        "trap_div_loop_edges",
        include_str!("../cases/case_trap_div_loop.rs"),
        &[(5, 4), (5, 3), (5, 0), (7, 9), (5, 10), (5, 13)],
    );
}

/// `checked_*(..).unwrap()` at the exact 32-bit overflow boundaries, next to
/// the `saturating_` / `unwrap_or` / `map_or` siblings that must not trap
/// there — the release guest's wrapping arithmetic makes the `checked_` form
/// the only panic source.
#[test]
fn trap_checked_arith() {
    run_case_traps("trap_checked_arith", include_str!("../cases/case_trap_checked_arith.rs"));
}

/// Pinned rows for `trap_checked_arith`: `u32::MAX - 1` (no trap) and
/// `u32::MAX` (`checked_add` traps), `input2 == 0` (`checked_sub` traps),
/// `i32::MIN` (`checked_neg` traps) and `i32::MIN + 1` (no trap), and the
/// origin.
#[test]
fn trap_checked_arith_edges() {
    run_case_traps_with_inputs(
        "trap_checked_arith_edges",
        include_str!("../cases/case_trap_checked_arith.rs"),
        &[
            (0xffff_fffe, 1),
            (0xffff_ffff, 1),
            (5, 0),
            (0x8000_0000, 1),
            (0x8000_0001, 1),
            (0, 1),
        ],
    );
}

/// The 64-bit `checked_*(..).unwrap()` boundaries: shift counts at and past
/// the width, `checked_add` at `u64::MAX`, and an overflowing `checked_pow`,
/// with `wrapping_shl` / `rotate_right` as the masking siblings that must
/// not trap at the same counts.
#[test]
fn trap_checked_wide() {
    run_case_traps("trap_checked_wide", include_str!("../cases/case_trap_checked_wide.rs"));
}

/// Pinned rows for `trap_checked_wide`: the last legal shift count (63), the
/// first illegal one (64), the largest (79), `u64::MAX` (`checked_add`
/// traps), `0xffff^5` (`checked_pow` overflows) and `0xffff^4` (it does
/// not), and the zero row.
#[test]
fn trap_checked_wide_edges() {
    run_case_traps_with_inputs(
        "trap_checked_wide_edges",
        include_str!("../cases/case_trap_checked_wide.rs"),
        &[
            (1, 63),
            (1, 64),
            (1, 79),
            (0xffff_ffff, 0xffff_ffff),
            (0xffff, 5),
            (0xffff, 4),
            (0, 0),
        ],
    );
}

/// `TryFrom` narrowing conversions panicking through `unwrap`, one per
/// destination width, each reading its own field of the inputs so its
/// boundary is reachable alone.
#[test]
fn trap_try_from() {
    run_case_traps("trap_try_from", include_str!("../cases/case_trap_try_from.rs"));
}

/// Pinned rows for `trap_try_from`: `u8` at 255 and 256, `i8` at 127
/// (0x00e3_00ff) and 128 (0x00e4_00ff), `i16` at 32767 and 32768, and
/// `u32::try_from(i32)` at 0 (0x00fa_0000) and -1 (0x00fb_0000).
#[test]
fn trap_try_from_edges() {
    run_case_traps_with_inputs(
        "trap_try_from_edges",
        include_str!("../cases/case_trap_try_from.rs"),
        &[
            (255, 1),
            (256, 1),
            (0x00e4_00ff, 1),
            (0x00e3_00ff, 1),
            (255, 32767),
            (255, 32768),
            (255, 0x00fb_0000),
            (255, 0x00fa_0000),
        ],
    );
}

/// The `core` validating constructors panicking through `unwrap`/`expect`:
/// `str::from_utf8` on a corrupted continuation byte, `char::from_u32` on a
/// surrogate, `NonZeroU32::new` on zero, and `Result::unwrap` on a
/// hand-written `Err`.
#[test]
fn trap_utf8_nonzero() {
    run_case_traps("trap_utf8_nonzero", include_str!("../cases/case_trap_utf8_nonzero.rs"));
}

/// Pinned rows for `trap_utf8_nonzero`, one per panic site: a clean row
/// (1, 1), invalid UTF-8 (7, 1), `NonZeroU32::new(0)` (4, 1), the first
/// surrogate (1, 0xd800) and the scalar just below it (1, 0xd7ff), the
/// `Err` row (1, 3), the largest scalar value (1, 0x10ffff) and its wrap to
/// zero (1, 0x110000).
#[test]
fn trap_utf8_nonzero_edges() {
    run_case_traps_with_inputs(
        "trap_utf8_nonzero_edges",
        include_str!("../cases/case_trap_utf8_nonzero.rs"),
        &[
            (1, 1),
            (7, 1),
            (4, 1),
            (1, 0xd800),
            (1, 0xd7ff),
            (1, 3),
            (1, 0x10_ffff),
            (1, 0x11_0000),
        ],
    );
}

/// The assertion macros: `debug_assert!` / `debug_assert_eq!` are compiled
/// out of a release guest and must not trap even with false predicates,
/// while `assert!` / `assert_eq!` / `assert_ne!` must trap on both targets
/// exactly when theirs fail.
#[test]
fn trap_asserts() {
    run_case_traps("trap_asserts", include_str!("../cases/case_trap_asserts.rs"));
}

/// Pinned rows for `trap_asserts`: both divisibility flags true (0, 0), the
/// `assert!` row (13, 0), the `assert_ne!` row (42, 0), a matching non-zero
/// pair (7, 0), a mismatched `assert_eq!` (1, 0), and a clean pair (1, 1) —
/// the `debug_assert!`s are false on every one of them.
#[test]
fn trap_asserts_edges() {
    run_case_traps_with_inputs(
        "trap_asserts_edges",
        include_str!("../cases/case_trap_asserts.rs"),
        &[(0, 0), (13, 0), (42, 0), (7, 0), (1, 0), (1, 1)],
    );
}

/// A dense `match` over a wrapped selector — the `br_table` shape — with
/// three panicking arms (`panic!`, `unreachable!()`, `todo!()`) among five
/// returning ones.
#[test]
fn trap_match_arms() {
    run_case_traps("trap_match_arms", include_str!("../cases/case_trap_match_arms.rs"));
}

/// Pinned rows for `trap_match_arms`, one per arm that matters: arm 0
/// (returns), arm 3 (`panic!`), arm 5 (`unreachable!()`), arm 7 (`todo!()`),
/// the wrap back to arm 0 (selector 8), and arm 6.
#[test]
fn trap_match_arms_edges() {
    run_case_traps_with_inputs(
        "trap_match_arms_edges",
        include_str!("../cases/case_trap_match_arms.rs"),
        &[(0, 9), (3, 9), (5, 9), (7, 9), (8, 9), (6, 9)],
    );
}

/// A trap two `#[inline(never)]` frames down, in the SECOND of two calls to
/// the same helper: the first call always returns and its result feeds the
/// second, so the trap sits behind a completed call.
#[test]
fn trap_deep_helper() {
    run_case_traps("trap_deep_helper", include_str!("../cases/case_trap_deep_helper.rs"));
}

/// Pinned rows for `trap_deep_helper`: the last valid index (5), the first
/// invalid one (6), the largest (8), the origin, and the wraps back into
/// range (9 % 9 = 0, 14 % 9 = 5) plus the next trap (15 % 9 = 6).
#[test]
fn trap_deep_helper_edges() {
    run_case_traps_with_inputs(
        "trap_deep_helper_edges",
        include_str!("../cases/case_trap_deep_helper.rs"),
        &[(5, 0), (6, 0), (8, 0), (0, 0), (9, 0), (14, 0), (15, 0)],
    );
}

/// A trap behind a `call_indirect`: the panicking operation is picked out of
/// a function-pointer table read through `core::hint::black_box`, which
/// keeps the dispatch indirect instead of the switch of direct calls the
/// nightly-2026-09-01 guest toolchain devirtualizes it into.
#[test]
fn trap_fnptr_dispatch() {
    run_case_traps("trap_fnptr_dispatch", include_str!("../cases/case_trap_fnptr_dispatch.rs"));
}

/// Pinned rows for `trap_fnptr_dispatch`: a returning slot (0), the
/// asserting slot with a failing (2, 1) and a passing (2, 2) argument, the
/// indexing slot with an in-range (3, 3) and an out-of-range (3, 4)
/// argument, and the rotating slot.
#[test]
fn trap_fnptr_dispatch_edges() {
    run_case_traps_with_inputs(
        "trap_fnptr_dispatch_edges",
        include_str!("../cases/case_trap_fnptr_dispatch.rs"),
        &[(0, 1), (2, 1), (2, 2), (3, 3), (3, 4), (1, 0)],
    );
}

/// Nested loops whose inner trip count comes from a `static` table, so the
/// index that eventually leaves the buffer depends on a data-segment lookup
/// as well as on both inputs, and the trap lands on the last trip of the
/// last outer iteration.
#[test]
fn trap_loop_late() {
    run_case_traps("trap_loop_late", include_str!("../cases/case_trap_loop_late.rs"));
}

/// Pinned rows for `trap_loop_late`: at the shortest inner bound the largest
/// surviving base (4, 2) and the first trapping one (5, 2), at the longest
/// bound the largest surviving base (2, 0) and the first trapping one
/// (3, 0), the origin, a base that traps early (6, 2), and the wrapped
/// limit index (0, 4).
#[test]
fn trap_loop_late_edges() {
    run_case_traps_with_inputs(
        "trap_loop_late_edges",
        include_str!("../cases/case_trap_loop_late.rs"),
        &[(4, 2), (5, 2), (0, 0), (3, 0), (2, 0), (6, 2), (0, 4)],
    );
}

/// The sharpest bounds check the harness can build: the array length is the
/// index modulus minus one, so exactly ONE of the sixteen index values is out
/// of range. A guard folded away, widened by one, or compiled as `<=`
/// instead of `<` shows up here and nowhere else.
#[test]
fn trap_bounds_single() {
    run_case_traps("trap_bounds_single", include_str!("../cases/case_trap_bounds_single.rs"));
}

/// Pinned rows for `trap_bounds_single`: the last valid index (14), the only
/// invalid one (15) and two of its wraps (31, `u32::MAX`), plus index 0
/// reached directly and through a wrap.
#[test]
fn trap_bounds_single_edges() {
    run_case_traps_with_inputs(
        "trap_bounds_single_edges",
        include_str!("../cases/case_trap_bounds_single.rs"),
        &[(14, 1), (15, 1), (31, 1), (0, 0), (16, 1), (0xffff_ffff, 1)],
    );
}

/// Trap parity in the other direction: three panics (an explicit `panic!`, a
/// zero divisor and an out-of-range index) all behind the same cross-modulus
/// contradiction, so NEITHER target may trap on any input. A MASM-side trap
/// here means a live branch was folded the wrong way.
#[test]
fn trap_dead_guard() {
    run_case_traps("trap_dead_guard", include_str!("../cases/case_trap_dead_guard.rs"));
}

/// Pinned rows for `trap_dead_guard`, all of which must return: the near
/// misses that satisfy one half of the contradiction — `h % 6 == 5` (5, 11,
/// 29) and `h % 3 == 0` (3, 6) — plus `h == 0` and a high-entropy pair.
#[test]
fn trap_dead_guard_edges() {
    run_case_traps_with_inputs(
        "trap_dead_guard_edges",
        include_str!("../cases/case_trap_dead_guard.rs"),
        &[(5, 0), (11, 0), (3, 0), (6, 0), (29, 0), (0, 0), (0xaaaa_aaaa, 1)],
    );
}

/// A trapping edge created inside a high-pressure region: eight u64 words
/// live across a six-trip ARX loop, then a bounds check whose index depends
/// on the loop's final state — so the trap cannot be hoisted above the work,
/// and the spill analysis and operand scheduler have to solve a region that
/// contains it.
#[test]
fn trap_spill_pressure() {
    run_case_traps("trap_spill_pressure", include_str!("../cases/case_trap_spill_pressure.rs"));
}

/// Pinned rows for `trap_spill_pressure`: the last base index that is in
/// range whatever the loop's low bit says (8), the first that traps (9), the
/// largest (12), the origin, the wrap back to 0 (13), and two rows where the
/// loop's low bit decides between them.
#[test]
fn trap_spill_pressure_edges() {
    run_case_traps_with_inputs(
        "trap_spill_pressure_edges",
        include_str!("../cases/case_trap_spill_pressure.rs"),
        &[(8, 0), (9, 0), (12, 0), (0, 0), (13, 0), (8, 1), (9, 1)],
    );
}

/// A loop whose ONLY exit is the trap for the inputs that never hit its
/// break: the bounds-check panic is the back edge's successor, which the
/// control-flow lifting has to keep as an `unreachable` terminator inside
/// the region rather than as a normal loop exit.
#[test]
fn trap_only_exit() {
    run_case_traps("trap_only_exit", include_str!("../cases/case_trap_only_exit.rs"));
}

/// Pinned rows for `trap_only_exit`: three rows that leave through the trap
/// (0, 3), (1, 1), (2, 3) and four that leave through the break, at each of
/// the three step sizes and from two different start indices.
#[test]
fn trap_only_exit_edges() {
    run_case_traps_with_inputs(
        "trap_only_exit_edges",
        include_str!("../cases/case_trap_only_exit.rs"),
        &[(0, 3), (0, 1), (1, 0), (2, 2), (0, 0), (1, 1), (2, 3)],
    );
}

/// A trap at the bottom of a four-deep loop nest, with an accumulator
/// escaping each level: the escaping values become region-op result columns
/// when cfg-to-scf lifts the nest, so the trapping edge has to survive the
/// same lifting that produces the wide result columns behind the known spill
/// classes.
///
/// The nest is deliberately one level shallower than the first draft. At
/// five levels it stops COMPILING at `--optimize=max` — `failed to schedule
/// operands: [%240, %374] for inst 'arith.rotl' with error: NoSolution,
/// constraints: [Move, Copy]` over a full 16-entry window at
/// codegen/masm/src/lower/lowering.rs:109, the F2 arity-2 solver gap
/// (`spills::rotl_window`). A sibling with the index masked into range
/// (`t[k & 15]`, no trapping edge at all) panics identically, so the trap
/// edge is NOT what makes the nest unschedulable: F2 is reached by the nest
/// alone and this case would be a duplicate reproducer, not a new shape.
#[test]
fn trap_deep_nest() {
    run_case_traps("trap_deep_nest", include_str!("../cases/case_trap_deep_nest.rs"));
}

/// Pinned rows for `trap_deep_nest`: the largest base whose deepest trip
/// still lands on the last element, the first that leaves the array, the
/// largest, the origin, an interior base, and the wrap back to 0.
#[test]
fn trap_deep_nest_edges() {
    run_case_traps_with_inputs(
        "trap_deep_nest_edges",
        include_str!("../cases/case_trap_deep_nest.rs"),
        &[(5, 1), (6, 1), (0, 0), (3, 3), (9, 2), (17, 1)],
    );
}

/// Documented probe, NOT a trap-parity case: Miden does not enforce wasm
/// linear-memory bounds. The case reads a `u32` at 256 MiB past the guest's
/// 17-page memory — undefined behaviour in Rust, so the native side is not a
/// language-level oracle, but the wasm semantics are unambiguous.
///
/// `native trap (native: signal 11), masm value 5` for inputs (0, 1): the
/// host segfaults on the unmapped address while Miden reads zero and
/// returns. wasmtime on the same harness-built wasm
/// (`target/miden_test_shared/wasm32-wasip1/release/
/// differential_trap_oob_read.wasm`) traps: "memory fault at wasm address
/// 0x100ffff0 in linear memory of size 0x110000 / wasm trap: out of bounds
/// memory access". So a wasm-level out-of-bounds access is silent on Miden —
/// the same missing enforcement `deep_overrun` (frames.rs) hits from the
/// other direction, where a wrapped shadow-stack address only trips the VM's
/// u32 range assertion by accident. Kept ignored as documentation: the trap
/// oracle cannot pass until Miden bounds-checks linear memory, and no other
/// case in this module depends on it.
#[test]
#[ignore = "Miden does not bounds-check linear memory: native trap (signal 11) / wasmtime `out of \
            bounds memory access` vs masm value 5 for inputs (0, 1)"]
fn trap_oob_read() {
    run_case_traps_with_inputs(
        "trap_oob_read",
        include_str!("../cases/case_trap_oob_read.rs"),
        &[(0, 1), (7, 2)],
    );
}

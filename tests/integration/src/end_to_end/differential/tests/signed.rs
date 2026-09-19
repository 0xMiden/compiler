//! Signed comparisons, division/remainder, shifts, and the signed widening-multiply family.

use super::super::harness::{run_case, run_case_with_inputs};

/// Signed widening shapes (the corpus otherwise never creates `arith.sext`):
/// extend_i32_s, extend8/16/32_s, and `i64.mul_wide_s` whose constant
/// multiplicand folds via `Sext::fold`'s I128 arm.
///
/// Formerly ignored (i1288): inputs (3022925119, 3340151117) diverged
/// (native 3550407903 vs masm 3550391763). Root cause (2026-09-02, verified
/// standalone without the Miden compiler): the GUEST toolchain — rustc
/// 1.97.0-nightly / LLVM 22.1.4 with `+wide-arithmetic` — sinks the
/// `i64.mul_wide_s` below a `local.get` of its own high word, so the wasm
/// reads a zero-initialised local and the `(hi as i16) as i64` term vanishes
/// (the same LLVM defect as the then-ignored `checked_mul_i64` /
/// `sat_mul_i64` / `pow_i64`, the F9 family). On nightly-2026-04-30 it passed
/// ONLY because the harness builds guests with `debug = 2`: the
/// variable-location records pinned the multiply's definitions and blocked the
/// sink (`-C debuginfo=1` still miscompiled). The nightly-2026-09-01 toolchain
/// fixed the family, and the campaign-31 sweep (2026-09-17) has this case and
/// its `_repro` twin passing at `FUZZA_GUEST_DEBUG=0` as well — so the DWARF
/// masking is no longer load-bearing and the test is a plain `arith.sext`
/// guard again.
#[test]
fn sext_shapes() {
    run_case("sext_shapes", include_str!("../cases/case_sext_shapes.rs"));
}

/// Pinned regression twin for `sext_shapes`: the exact `(input1, input2)`
/// pair that once diverged (i1288), so that pair is asserted on every run
/// rather than only when proptest happens to draw it.
#[test]
fn sext_shapes_repro() {
    run_case_with_inputs(
        "sext_shapes_repro",
        include_str!("../cases/case_sext_shapes.rs"),
        &[(3022925119, 3340151117)],
    );
}

/// Sign-extension width conversions (extend8/16/32_s, extend_i32_s) —
/// `wasm.SignExtend` lowers to `trunc(src)` + `sext(dst)`, covering
/// `trunc_int32`/`trunc_int64` small-width arms, `sext_smallint`
/// (8/16 -> 32/64), and `sext_int32(64)`; no i128 shapes.
#[test]
fn sext_widths() {
    run_case("sext_widths", include_str!("../cases/case_sext_widths.rs"));
}

/// Dynamic-by-dynamic `i64.mul_wide_s` — both operands sign-extended to i128
/// (`sext_int64(128)`, its only Rust-reachable producer) plus the signed
/// wide-multiply hi/lo recombination, without the constant operand of `sext_shapes`.
#[test]
fn mulwide_dyn() {
    run_case("mulwide_dyn", include_str!("../cases/case_mulwide_dyn.rs"));
}

/// `i64.mul_wide_s` with a positive constant multiplicand — `Sext::fold`
/// materializes an I128 immediate that the scheduler pushes via `push_i128`,
/// its only Rust-reachable producer.
#[test]
fn mulwide_fold() {
    run_case("mulwide_fold", include_str!("../cases/case_mulwide_fold.rs"));
}

/// Signed i32 comparisons (`< <= > >=`) over both-sign operands feeding
/// branches and selects — the `Type::I32` arms of the `binary.rs` compare
/// dispatchers (`::intrinsics::i32::is_lt/is_lte/is_gt/is_gte`).
#[test]
fn i32_scmp() {
    run_case("i32_scmp", include_str!("../cases/case_i32_scmp.rs"));
}

/// Signed i64 comparisons (`< <= > >=`) over both-sign operands feeding
/// branches and selects — the `Type::I64` arms of the `binary.rs` compare
/// dispatchers and the `lt_i64`/`lte_i64`/`gt_i64`/`gte_i64` emitters
/// (`::intrinsics::i64::{lt,lte,gt,gte}`).
#[test]
fn i64_scmp() {
    run_case("i64_scmp", include_str!("../cases/case_i64_scmp.rs"));
}

/// Signed i32 division/remainder in all four sign combinations with
/// by-construction-safe dynamic divisors — `checked_div`'s I32 arm ->
/// `checked_div_i32` and `wasm.I32RemS` -> `wrapping_mod` ->
/// `wrapping_mod_i32` (truncate-toward-zero remainder signs).
#[test]
fn i32_sdiv() {
    run_case("i32_sdiv", include_str!("../cases/case_i32_sdiv.rs"));
}

/// Non-strict signed compares (`<=`/`>=`, both widths) materialized as
/// boolean VALUES — branches/selects always canonicalize to strict compares,
/// so this value form is the only producer of `i32.le_s/ge_s`/`i64.le_s/ge_s`
/// and the `lte`/`gte` I32 arms + `lte_i64`/`gte_i64` emitters.
#[test]
fn scmp_bool() {
    run_case("scmp_bool", include_str!("../cases/case_scmp_bool.rs"));
}

/// Arithmetic shift right (i32/i64) with dynamic masked counts and constant
/// counts — the `Type::I32`/`Type::I64` arms of the `shr` dispatcher ->
/// `shr_i32`/`shr_i64` (`::intrinsics::{i32,i64}::checked_shr`); the
/// `shr_imm_*` variants have no non-test callers.
#[test]
fn i_ashr() {
    run_case("i_ashr", include_str!("../cases/case_i_ashr.rs"));
}

/// Signed i64 division with by-construction-safe dynamic divisors (positive
/// and negative) — `checked_div`'s I64 arm -> `checked_div_i64`
/// (`::intrinsics::i64::checked_div`, which execs miden-core-lib `u64::div`).
#[test]
fn i64_sdiv() {
    run_case("i64_sdiv", include_str!("../cases/case_i64_sdiv.rs"));
}

/// Signed division/remainder GUARD shapes with fully dynamic divisors that
/// reach 0, -1 and MIN: i32 checked_div/checked_rem/wrapping_div/wrapping_rem/
/// overflowing_div/checked_div_euclid/checked_rem_euclid and i64 checked_div/
/// wrapping_div/overflowing_div — LLVM's `rhs == 0` / `rhs == -1 && lhs ==
/// MIN` branch arms around the bare `div_s`/`rem_s` (`::intrinsics::i32::
/// checked_div`/`wrapping_mod`, `::intrinsics::i64::checked_div`), which the
/// by-construction-safe divisors of sdiv_bounds/i32_sdiv/i64_sdiv never form.
#[test]
fn sdiv_guards() {
    run_case("sdiv_guards", include_str!("../cases/case_sdiv_guards.rs"));
}

/// Pinned edge grid for `sdiv_guards`: (0x80000000, 0xFFFFFFFF) is
/// i32::MIN / -1 AND i64::MIN / -1 at once (None / wrapping MIN / rem 0 /
/// overflow flag); (x, 0) divides by zero everywhere; MIN/1, MIN/MIN, -1/MIN
/// (rem_euclid -1 + |MIN| == MAX), (MIN+1)/-1 == MAX, MAX/-1, 0/-1, 1/MIN, and
/// the four sign combinations of 7 and 2 (truncating vs euclidean quotients).
#[test]
fn sdiv_guards_edges() {
    run_case_with_inputs(
        "sdiv_guards_edges",
        include_str!("../cases/case_sdiv_guards.rs"),
        &[
            (0x80000000, 0xffffffff),
            (0x80000000, 0),
            (0x80000000, 1),
            (0x80000000, 0x80000000),
            (0x80000001, 0xffffffff),
            (0xffffffff, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0xffffffff),
            (0, 0xffffffff),
            (1, 0x80000000),
            (7, 0xfffffffe),
            (0xfffffff9, 2),
            (0xfffffff9, 0xfffffffe),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Sign-extension / truncation chains in orders the corpus never formed:
/// i8 -> u64, u8 -> i64, i16 -> i64 then `>> 40`, extension after arithmetic
/// that set the high bits, `i64::from(i32) * i64::from(i32)` at MIN/MAX,
/// sext then logical shift, i8 * i8 in i32, the high u64 limb through i8 ->
/// i64, an i32 -> i64 -> i32 round trip, and i64 x i16 products in i128.
#[test]
fn ext_chains() {
    run_case("ext_chains", include_str!("../cases/case_ext_chains.rs"));
}

/// Pinned edge grid for `ext_chains`: i8/i16 boundary bytes and halfwords
/// (0x7F/0x80/0xFF, 0x7FFF/0x8000/0xFFFF, plus 0x100/0x10000 which truncate
/// to 0), i32::MIN * i32::MIN and MIN * -1 products, MAX * MAX, all-ones,
/// zero, and truncation-before-extension rows (0xFF80 -> -128).
#[test]
fn ext_chains_edges() {
    run_case_with_inputs(
        "ext_chains_edges",
        include_str!("../cases/case_ext_chains.rs"),
        &[
            (0x7f, 0x7fff),
            (0x80, 0x8000),
            (0xff, 0xffff),
            (0x100, 0x10000),
            (0x80000000, 0x80000000),
            (0x80000000, 0xffffffff),
            (0xffffffff, 0x80000000),
            (0x7fffffff, 0x7fffffff),
            (0xffffffff, 0xffffffff),
            (0, 0),
            (0xff80, 0xffff8000),
            (0x8000, 0x80),
            (0x12345678, 0x9abcdef0),
        ],
    );
}

/// Comparison chains at sign and limb boundaries: the same 64-bit patterns
/// compared signed (`::intrinsics::i64::lt/gt` sign-difference arm) and
/// unsigned side by side, `Ord::cmp` on i64/i32/u32, i64 min/max/clamp, u64
/// max, and i128 compares whose operands differ only in the low limb or by
/// exactly one in the high limb (both legs of the two-limb legalization).
#[test]
fn cmp_chains() {
    run_case("cmp_chains", include_str!("../cases/case_cmp_chains.rs"));
}

/// Pinned edge grid for `cmp_chains`: mirrored sign halves ((0x80000000,
/// 0x7FFFFFFF) and its swap), i64::MIN against 2^31 and against itself,
/// -1 against -1 / 0 / 2^32-1, MAX against -2^31-1, and the i128 high-limb
/// wrap rows (a == i64::MAX makes a + 1 wrap to i64::MIN in the helper).
#[test]
fn cmp_chains_edges() {
    run_case_with_inputs(
        "cmp_chains_edges",
        include_str!("../cases/case_cmp_chains.rs"),
        &[
            (0x80000000, 0x7fffffff),
            (0x7fffffff, 0x80000000),
            (0x80000000, 0),
            (0, 0x80000000),
            (0xffffffff, 0xffffffff),
            (0, 0),
            (0xffffffff, 0),
            (0, 0xffffffff),
            (0x7fffffff, 0xffffffff),
            (0x80000000, 0x80000000),
            (1, 0xffffffff),
            (0x80000001, 0x7ffffffe),
            (0x7fffffff, 0x7fffffff),
            (0x80000000, 1),
        ],
    );
}

/// Formerly `#[ignore]`d (F9), simplest form of the guest-toolchain
/// `+wide-arithmetic` miscompile: a non-inlined `i64::checked_mul`. rustc
/// 1.97.0-nightly (c935696dd 2026-04-29, LLVM 22.1.4) emitted the overflow
/// test `hi == (lo >> 63)` with the `local.get` of the `i64.mul_wide_s` hi
/// result placed BEFORE the multiply that defines it (WAT: `local.get 2` ...
/// `i64.mul_wide_s` `local.set 2` ... `i64.eq` `br_if`), so the wasm compared
/// the parameter `y` with `lo >> 63` instead of `hi` and (MAX, MAX) came back
/// as masm 0 against native 3758096385.
///
/// Fixed by the guest toolchain bump to nightly-2026-09-01 (fd6f5b171), not by
/// midenc; re-verified 2026-09-17 on the pinned inputs and the random pairs.
/// The evidence that the wasm — not the compiler — changed: `wasmtime run -W
/// wide-arithmetic=y --invoke entrypoint differential_checked_mul_i64.wasm -1
/// -1` now returns 3758096385, the native value, where it used to return the
/// MASM one; and the wasm still contains the `i64.mul_wide_s`, so this case
/// still exercises the wide multiply rather than having lost the shape.
///
/// Bounded by passing siblings: inlined `if let Some(x) = a.checked_mul(b)`
/// and `overflowing_mul` flag branches (LLVM stackifies those in a valid
/// order), u64 / u32 / i32 / u128 / i128 checked/overflowing/saturating
/// multiplies (ovf_mul, u64_sat_forms; `hi != 0` needs no second operand),
/// and the high-word-only / branch-on-constant uses of dynamic
/// `i64.mul_wide_s` products (mul_hi_only).
#[test]
fn checked_mul_i64() {
    run_case("checked_mul_i64", include_str!("../cases/case_checked_mul_i64.rs"));
}

/// Regression guard for the `checked_mul_i64` divergence, formerly the pinned
/// `#[ignore]`d twin: -1 * -1 through the helper (native Some(1) folded with
/// the low word; the old wasm read y == -1 as the hi word and reported
/// overflow -> None). Kept pinned so the fix is checked on the exact rows that
/// used to fail, not only on random draws.
#[test]
fn checked_mul_i64_repro() {
    run_case_with_inputs(
        "checked_mul_i64_repro",
        include_str!("../cases/case_checked_mul_i64.rs"),
        &[(0xffffffff, 0xffffffff), (0x80000000, 0x80000000), (32, 32)],
    );
}

/// Formerly `#[ignore]`d (F9), saturating form of the `checked_mul_i64`
/// defect: `i64::saturating_mul` on dynamic operands. On the 2026-04-30
/// toolchain the overflow test `hi == (lo >> 63)` read a stale zero local (or
/// the PREVIOUS product's hi word) instead of the `i64.mul_wide_s` hi result,
/// so overflowing products with a non-negative low word came back unsaturated
/// and in-range products after an overflowing one saturated — (32, 32) gave
/// masm 1025 against native 2147483649.
///
/// Fixed by the nightly-2026-09-01 guest toolchain; re-verified 2026-09-17
/// (wasmtime on the harness-built wasm returns 2147483649 for (32, 32) and 0
/// for the pinned (1508586408, 1) row, both the native values, and the two
/// `i64.mul_wide_s` ops are still in the wasm).
#[test]
fn sat_mul_i64() {
    run_case("sat_mul_i64", include_str!("../cases/case_sat_mul_i64.rs"));
}

/// Regression guard for the `sat_mul_i64` divergence, formerly the pinned
/// `#[ignore]`d twin: the distinct-operand overflow row (1508586408, 1)
/// (native 0: MAX xor the rotated 2^62; the old wasm returned 1508586409, the
/// unsaturated low word, and the x * x form then saturated on the FIRST
/// product's stale hi word) and the i64::MAX * i64::MAX x * x row
/// (u32::MAX, u32::MAX).
#[test]
fn sat_mul_i64_repro() {
    run_case_with_inputs(
        "sat_mul_i64_repro",
        include_str!("../cases/case_sat_mul_i64.rs"),
        &[(1508586408, 1), (0xffffffff, 0xffffffff)],
    );
}

/// Formerly `#[ignore]`d (F9), loop form of the `checked_mul_i64` defect:
/// `i64::checked_pow` with a dynamic exponent. Its square-and-multiply loop
/// calls `checked_mul` twice per iteration, and the 2026-04-30 LLVM emitted
/// each overflow test `hi != (lo >> 63)` with the `local.get` of the
/// `i64.mul_wide_s` hi result placed before the multiply, so every test read
/// the previous iteration's hi word (zero on the first) and (-1)^7 came back
/// as None. It failed at the default configuration, `--optimize=size-min` and
/// `--optimize=max`.
///
/// Fixed by the nightly-2026-09-01 guest toolchain; re-verified 2026-09-17
/// (wasmtime returns 2147483713 for (2147483648, 65) and 0 for the pinned
/// all-ones row — the native values — and both `i64.mul_wide_s` ops survive in
/// the wasm). Bounded by the passing u32/i32/u64 `checked_pow` loops
/// (int_logs).
#[test]
fn pow_i64() {
    run_case("pow_i64", include_str!("../cases/case_pow_i64.rs"));
}

/// Regression guard for the `pow_i64` divergence, formerly the pinned
/// `#[ignore]`d twin: (-1)^7 via the all-ones row (native 0, old masm
/// 4076380402), plus a positive-base overflow row.
#[test]
fn pow_i64_repro() {
    run_case_with_inputs(
        "pow_i64_repro",
        include_str!("../cases/case_pow_i64.rs"),
        &[(0xffffffff, 0xffffffff), (0x12345678, 3)],
    );
}

/// Signed i64 remainder with a dynamic divisor exercises the dedicated Wasm remainder lowering.
#[test]
fn i64_srem() {
    run_case("i64_srem", include_str!("../cases/case_i64_srem.rs"));
}

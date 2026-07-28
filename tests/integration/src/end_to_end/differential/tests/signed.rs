//! Signed comparisons, division/remainder, shifts, and the signed widening-multiply family.

use super::super::harness::{run_case, run_case_with_inputs};

/// Signed widening shapes (the corpus otherwise never creates `arith.sext`):
/// extend_i32_s, extend8/16/32_s, and `i64.mul_wide_s` whose constant
/// multiplicand folds via `Sext::fold`'s I128 arm.
///
/// Passing siblings bound the divergence: `sext_widths` (pure extend chains),
/// `mulwide_dyn` (dynamic-by-dynamic `mul_wide_s`), and `mulwide_fold`
/// (positive-constant fold) all pass — suspicion falls on the
/// negative-constant multiplicand path or a shape interaction.
#[test]
#[ignore = "native/masm divergence: inputs (3022925119, 3340151117) -> native 3550407903, masm \
            3550391763; signed i128 widening-multiply/sign-extension shapes"]
fn sext_shapes() {
    run_case("sext_shapes", include_str!("../cases/case_sext_shapes.rs"));
}

/// Deterministic reproducer for the `sext_shapes` divergence: pins the exact
/// `(input1, input2)` pair the fuzzer flagged, so the mismatch fails reliably
/// on that input rather than only when proptest happens to draw it.
#[test]
#[ignore = "native/masm divergence on pinned input (3022925119, 3340151117): native 3550407903 vs \
            masm 3550391763; deterministic reproducer for the sext_shapes divergence"]
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
/// wide-multiply hi/lo recombination, without the constant-fold shape of the
/// ignored sext_shapes case.
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

/// Reproducer for a compile-time gap: signed 64-bit `%` with a dynamic
/// divisor — `arith.Mod` on I64 reaches `checked_mod`, whose dispatch has no
/// I64 arm (and no wasm.I64RemS op or i64 mod intrinsic exists to back one).
#[test]
#[ignore = "compile-time compiler panic: 'not implemented: checked_mod for i64 is not supported' \
            (codegen/masm/src/emit/binary.rs:665); i64 % with a dynamic divisor cannot compile"]
fn i64_srem() {
    run_case("i64_srem", include_str!("../cases/case_i64_srem.rs"));
}

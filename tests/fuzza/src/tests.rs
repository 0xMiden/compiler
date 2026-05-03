//! Fuzz cases. One `#[test]` per file under `cases/`, driven by `run_case`.

use super::run_case;

#[test]
fn add() {
    run_case("add", include_str!("cases/case_add.rs"));
}

#[test]
fn xor() {
    run_case("xor", include_str!("cases/case_xor.rs"));
}

/// Non-commutative — exercises argument ordering (`input1 - input2`).
#[test]
fn sub() {
    run_case("sub", include_str!("cases/case_sub.rs"));
}

#[test]
fn branchy() {
    run_case("branchy", include_str!("cases/case_branchy.rs"));
}

/// Exercises bounded loops with carried values and nested conditional control flow.
#[test]
fn while_carried() {
    run_case("while_carried", include_str!("cases/case_while_carried.rs"));
}

/// Exercises dense match/switch control flow, including wasm `br_table` translation.
#[test]
fn dense_match() {
    run_case("dense_match", include_str!("cases/case_dense_match.rs"));
}

/// Exercises nested loops, local breaks, and labelled non-local loop exits.
#[test]
fn nested_breaks() {
    run_case("nested_breaks", include_str!("cases/case_nested_breaks.rs"));
}

/// Exercises sparse/default-heavy switch control flow.
#[test]
fn sparse_match() {
    run_case("sparse_match", include_str!("cases/case_sparse_match.rs"));
}

/// Exercises compile-time translation of an unreachable panic edge.
#[test]
#[ignore = "fuzzer found a native/MASM divergence — inputs (363814857, 995348134) trigger a MASM \
            assertion (eqz) at cycle 67; needs investigation before re-enabling"]
fn unreachable_guard() {
    run_case("unreachable_guard", include_str!("cases/case_unreachable_guard.rs"));
}

#[test]
#[ignore = "fuzzer found a native/MASM divergence on wrapping_mul; e.g. inputs (530384503, \
            3296201177) trigger an intrinsic panic in i32.masm — needs investigation before \
            re-enabling"]
fn muladd() {
    run_case("muladd", include_str!("cases/case_muladd.rs"));
}

/// Exercises integer width conversions and per-width bit-counting/`bnot`
/// arms in `codegen/masm/src/emit/unary.rs`.
#[test]
fn widening() {
    run_case("widening", include_str!("cases/case_widening.rs"));
}

/// Exercises u32 bitwise / shift / rotate / comparison emitter arms in
/// `codegen/masm/src/emit/binary.rs`.
#[test]
#[ignore = "fuzzer found a native/MASM divergence — inputs (4146962468, 1369714330) trigger a MASM \
            assertion (eqz) at cycle 92; needs investigation before re-enabling"]
fn bitops() {
    run_case("bitops", include_str!("cases/case_bitops.rs"));
}

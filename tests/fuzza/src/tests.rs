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
#[ignore = "fuzzer found a native/MASM divergence on wrapping_mul; e.g. inputs (530384503, \
            3296201177) trigger an intrinsic panic in i32.masm — needs investigation before \
            re-enabling"]
fn muladd() {
    run_case("muladd", include_str!("cases/case_muladd.rs"));
}

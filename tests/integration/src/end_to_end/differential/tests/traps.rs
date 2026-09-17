//! Trap parity: cases that deliberately panic on some inputs, compared with
//! `run_case_traps` so that both targets must trap on exactly the same
//! inputs and agree on the value everywhere else.

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

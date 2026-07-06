// Loop whose carried values exercise the scf.while canonicalization
// patterns: two variables converging to the same yielded value
// (`while_remove_duplicated_results`), carried values dead after the loop
// (`while_unused_result`), and a loop-invariant value used in the body
// (`remove_loop_invariant_args_from_before_block`).
//
// NB: the trip count is `input2 % 97` — a bound LLVM will not fully
// unroll/peel (masking with a small power-of-two gets the loop peeled away
// entirely at opt-level 3).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let k = input1 | 1; // loop-invariant multiplier (odd, opaque to LLVM)
    let mut a = input1;
    let mut b = input2;
    let mut scratch = input2 ^ 0x9e37_79b9; // live in the loop, dead after it
    let mut i = 0u32;
    let n = input2 % 97;
    while i < n {
        let t = a.wrapping_add(b).wrapping_mul(k) ^ (scratch & 0xff);
        // Both `a` and `b` are assigned the same value, so the loop yields
        // the same SSA value in two result positions.
        a = t;
        b = t;
        scratch = scratch.rotate_left(5).wrapping_add(i);
        i = i.wrapping_add(1);
    }
    // `scratch` and `i` are dead here, leaving unused scf.while results.
    a.wrapping_add(b)
}

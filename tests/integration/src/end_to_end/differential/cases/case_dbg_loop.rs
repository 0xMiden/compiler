// Loop-carried NAMED accumulators plus a value computed before the loop but
// read only after it: DWARF location ranges must stay valid across backedges,
// and the pre-loop variable's debug liveness outlives its real liveness gap
// inside the loop. Drives multi-range location lists and schedule kill events
// (`build_location_schedule`), plus the per-iteration block-scoped `round`
// variable. The add/xor loop body collapses if LLVM unrolls it, keeping the
// shape clear of the known unroll-family scheduler panics. Debug info must
// never change semantics.

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let bound = input1 % 23;
    let preloop = input1.wrapping_mul(0x85eb_ca6b) ^ input2;
    let mut acc = input2 | 1;
    let mut steps = 0u32;
    let mut i = 0u32;
    while i < bound {
        let round = acc ^ i;
        acc = round.wrapping_add(0x9e37_79b9);
        steps = steps.wrapping_add(round & 7);
        i = i.wrapping_add(1);
    }
    acc.wrapping_add(steps) ^ preloop
}

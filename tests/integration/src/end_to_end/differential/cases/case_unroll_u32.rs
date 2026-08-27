// u32 variant of the unrolled mul-xor-rotate accumulator round: LLVM
// runtime-unrolls the `% 97`-bounded loop into one block of interleaved
// u32 mul/xor/rotl rounds with several `i+k` operands live at once. Unlike
// the u64 rounds (unroll_chain: NoSolution; unroll_rotmix: movdn
// out-of-range), the single-felt u32 chain SCHEDULES, pressing the operand
// scheduler's tactic interiors from just inside the solvable boundary.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1 | 1;
    let mut i: u32 = 0;
    while i < input2 % 97 {
        acc = (acc.wrapping_mul(33) ^ i).rotate_left(5);
        i = i.wrapping_add(1);
    }
    acc
}

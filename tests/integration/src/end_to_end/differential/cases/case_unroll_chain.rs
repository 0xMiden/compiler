// Minimal reproducer for an operand-scheduler NoSolution compile panic:
// LLVM runtime-unrolls this `% 97`-bounded loop 8x, producing one basic
// block whose body is the interleaved non-reassociable chain
// `((((acc*33)^i)*33)^(i+1))*33 ...` (eight mul-by-33 / xor-of-induction
// rounds plus eight induction increments). Scheduling that block's operands
// defeats every solver tactic. See the `unroll_chain` test for the full
// finding notes; the mul-only and xor-only bodies of the same loop compile
// and pass (their unrolled chains collapse).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = input2;
    let mut acc = input1;
    let mut i = 0u32;
    while i < x % 97 {
        acc = acc.wrapping_mul(33) ^ i;
        i = i.wrapping_add(1);
    }
    acc
}

// Minimal reproducer for a compile-time panic in the MASM operand-scheduler
// solution applier: LLVM runtime-unrolls this `% 97`-bounded
// mul-xor-rotate accumulator round (4x, vs 8x for the rotate-less
// unroll_chain round), and applying the solver's solution executes
// `Stack::movdn` with a position past the end of the model stack —
// `attempt to subtract with overflow` at codegen/masm/src/opt/operands/
// stack.rs:80. See the ignored test for the bounding variants.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1 as u64;
    let mut i: u32 = 0;
    while i < input2 % 97 {
        acc = (acc.wrapping_mul(33) ^ (i as u64)).rotate_left(5);
        i = i.wrapping_add(1);
    }
    (acc as u32) ^ ((acc >> 32) as u32)
}

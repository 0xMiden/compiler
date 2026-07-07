// Two return paths that LLVM tail-merges into one exit block carrying the
// return value as a block argument, plus an impossible trap exit that keeps
// the branch between exits alive through cfg-to-scf (mixed return-like
// kinds). The surviving cf.cond_br then has a successor with block
// arguments, exercising the successor-operand scheduling/renaming loops in
// the cf.cond_br MASM lowering.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1.wrapping_mul(0x0101_0101) ^ input2;
    if h & 0xf == 3 {
        return h.rotate_left(5).wrapping_add(input2);
    }
    // Impossible: h % 6 == 1 implies h is odd, h % 4 == 0 implies h is even.
    if h % 6 == 1 && h % 4 == 0 {
        panic!();
    }
    h.rotate_right(7).wrapping_sub(input1)
}

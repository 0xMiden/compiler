// Single-block window guard for the arity-2 operand scheduler. Eighteen
// masked rotate counts are first used on `y` (their bands become CSE-merged
// u32 SSA values that stay on the operand stack) and then on the multi-use
// `x` (one CSE-merged `load_local` with nineteen uses, Copy-constrained at
// every rotate but the last). LLVM schedules the `x` rotates ahead of the
// xor/add chain, so their results pile up above `x` and the counts: at
// eighteen counts the spill analysis keeps every problem in-contract and the
// case passes; at twenty counts the first `x` rotate sees its Move count at
// the bottom of a 15-felt window with the Copy `x` above it — the known
// arity-2 `NoSolution` (`rotl_window` class), reproduced here without any
// loop, dispatch, or cross-edge spill.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut r: u64 = y | 1;
    // First uses of the shared counts (y dies at the end of this chain).
    r ^= y.rotate_left(1);
    r = r.wrapping_add(y.rotate_left(3));
    r = r.wrapping_sub(y.rotate_left(5));
    r ^= y.rotate_left(7);
    r = r.wrapping_add(y.rotate_left(9));
    r = r.wrapping_sub(y.rotate_left(11));
    r ^= y.rotate_left(13);
    r = r.wrapping_add(y.rotate_left(15));
    r = r.wrapping_sub(y.rotate_left(17));
    r ^= y.rotate_left(19);
    r = r.wrapping_add(y.rotate_left(21));
    r = r.wrapping_sub(y.rotate_left(23));
    r ^= y.rotate_left(25);
    r = r.wrapping_add(y.rotate_left(27));
    r = r.wrapping_sub(y.rotate_left(29));
    r ^= y.rotate_left(31);
    r = r.wrapping_add(y.rotate_left(33));
    r = r.wrapping_sub(y.rotate_left(35));
    let x = ((input1 ^ 0x85eb_ca6b) as u64) | 1;
    // Second uses of the counts on the multi-use x, deepest count first.
    r ^= x.rotate_left(1);
    r = r.wrapping_add(x.rotate_left(3));
    r = r.wrapping_sub(x.rotate_left(5));
    r ^= x.rotate_left(7);
    r = r.wrapping_add(x.rotate_left(9));
    r = r.wrapping_sub(x.rotate_left(11));
    r ^= x.rotate_left(13);
    r = r.wrapping_add(x.rotate_left(15));
    r = r.wrapping_sub(x.rotate_left(17));
    r ^= x.rotate_left(19);
    r = r.wrapping_add(x.rotate_left(21));
    r = r.wrapping_sub(x.rotate_left(23));
    r ^= x.rotate_left(25);
    r = r.wrapping_add(x.rotate_left(27));
    r = r.wrapping_sub(x.rotate_left(29));
    r ^= x.rotate_left(31);
    r = r.wrapping_add(x.rotate_left(33));
    r = r.wrapping_sub(x.rotate_left(35));
    r ^= x.wrapping_mul(0x2545_f491_4f6c_dd1d);
    (r as u32) ^ ((r >> 32) as u32)
}

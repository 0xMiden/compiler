// Width-mix freight under an arity-2 problem: sixteen shared rotate counts
// alternate between a u64 and a u32 source (`y`/`y32`, then the multi-use
// `x`/`x32`), so the window holds an interleaving of one- and two-felt words
// (widened u32 rotates, u64 rotates, u32 bands) when the Copy-constrained
// `x` and `x32` are copied from below them. Passes: the scheduler's felt
// accounting (`effective_index`, multi-felt dup/movup/swap) is exact for
// mixed widths.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let y32 = input2.rotate_left(5) | 1;
    let mut r: u64 = y | 1;
    r ^= y.rotate_left(1);
    r = r.wrapping_add(y32.rotate_left(3) as u64);
    r ^= y.rotate_left(5);
    r = r.wrapping_add(y32.rotate_left(7) as u64);
    r ^= y.rotate_left(9);
    r = r.wrapping_add(y32.rotate_left(11) as u64);
    r ^= y.rotate_left(13);
    r = r.wrapping_add(y32.rotate_left(15) as u64);
    r ^= y.rotate_left(17);
    r = r.wrapping_add(y32.rotate_left(19) as u64);
    r ^= y.rotate_left(21);
    r = r.wrapping_add(y32.rotate_left(23) as u64);
    r ^= y.rotate_left(25);
    r = r.wrapping_add(y32.rotate_left(27) as u64);
    r ^= y.rotate_left(29);
    r = r.wrapping_add(y32.rotate_left(31) as u64);
    let x = ((input1 ^ 0x85eb_ca6b) as u64) | 1;
    let x32 = input1.rotate_left(9) | 1;
    r ^= x.rotate_left(1);
    r = r.wrapping_sub(x32.rotate_left(3) as u64);
    r ^= x.rotate_left(5);
    r = r.wrapping_sub(x32.rotate_left(7) as u64);
    r ^= x.rotate_left(9);
    r = r.wrapping_sub(x32.rotate_left(11) as u64);
    r ^= x.rotate_left(13);
    r = r.wrapping_sub(x32.rotate_left(15) as u64);
    r ^= x.rotate_left(17);
    r = r.wrapping_sub(x32.rotate_left(19) as u64);
    r ^= x.rotate_left(21);
    r = r.wrapping_sub(x32.rotate_left(23) as u64);
    r ^= x.rotate_left(25);
    r = r.wrapping_sub(x32.rotate_left(27) as u64);
    r ^= x.rotate_left(29);
    r = r.wrapping_sub(x32.rotate_left(31) as u64);
    r ^= x.wrapping_mul(0x2545_f491_4f6c_dd1d) ^ (x32 as u64);
    (r as u32) ^ ((r >> 32) as u32)
}

// Arity-1 operand scheduling under a full window: thirteen shared rotate
// counts and the accumulator sit above the multi-use `x` when `leading_zeros`
// consumes a copy of it. Arity-1 problems never enter the solver
// (`solve_and_apply` emits the dup/movup directly), and any operand of a
// <= 16-felt stack is addressable (dup.15 dup.15 for a bottom u64), so this
// ladder has no failure step; the case pins that.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let x = ((input1 ^ 0x85eb_ca6b) as u64) | 1;
    let mut r: u64 = y ^ x;
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
    r ^= x.leading_zeros() as u64;
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
    (r as u32) ^ ((r >> 32) as u32)
}

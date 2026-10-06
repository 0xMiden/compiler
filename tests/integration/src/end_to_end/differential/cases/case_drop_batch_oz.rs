// Twenty masked rotate count bands shared between the pre-loop code and the
// rotates of the loop-carried accumulator, with NO use after the loop, so at
// `--optimize=size-min` every band is dead at the function's `return`. The
// whole operand stack is unused there, which is the block emitter's
// whole-stack batch drop arm ("0 used operands out of 11" -> `dropn`) rather
// than the used/unused interleave arms `band_guard_oz` reaches. With no
// post-loop band use the count-band ladder has no window boundary: twenty
// bands compile and pass, where NINE is the limit as soon as one band is
// live past the loop (see `band_guard_oz`).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= n.rotate_left(7);
    acc = acc.wrapping_add(m.rotate_left(9));
    acc = acc.wrapping_sub(n.rotate_left(11));
    acc ^= m.rotate_left(13);
    acc = acc.wrapping_add(n.rotate_left(15));
    acc = acc.wrapping_sub(m.rotate_left(17));
    acc ^= n.rotate_left(19);
    acc = acc.wrapping_add(m.rotate_left(21));
    acc = acc.wrapping_sub(n.rotate_left(23));
    acc ^= m.rotate_left(25);
    acc = acc.wrapping_add(n.rotate_left(27));
    acc = acc.wrapping_sub(m.rotate_left(29));
    acc ^= n.rotate_left(31);
    acc = acc.wrapping_add(m.rotate_left(33));
    acc = acc.wrapping_sub(n.rotate_left(35));
    acc ^= m.rotate_left(37);
    acc = acc.wrapping_add(n.rotate_left(39));
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    while i < iters {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13) | 1;
        acc = acc.wrapping_add(acc.rotate_left(15));
        acc = acc.wrapping_sub(acc.rotate_left(17));
        acc ^= acc.rotate_left(19) | 1;
        acc = acc.wrapping_add(acc.rotate_left(21));
        acc = acc.wrapping_sub(acc.rotate_left(23));
        acc ^= acc.rotate_left(25) | 1;
        acc = acc.wrapping_add(acc.rotate_left(27));
        acc = acc.wrapping_sub(acc.rotate_left(29));
        acc ^= acc.rotate_left(31) | 1;
        acc = acc.wrapping_add(acc.rotate_left(33));
        acc = acc.wrapping_sub(acc.rotate_left(35));
        acc ^= acc.rotate_left(37) | 1;
        acc = acc.wrapping_add(acc.rotate_left(39));
        i = i.wrapping_add(1);
    }
    (acc as u32) ^ ((acc >> 32) as u32)
}

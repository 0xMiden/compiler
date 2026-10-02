// Reload placement across an `scf.while` that carries result columns:
// twelve counts shared between the pre-loop code and the loop body (dead
// after the loop) plus two counts used only before and after the loop
// (live through), with an in-loop `return` so cfg-to-scf gives the while a
// dispatch discriminator and a payload column. The loop-header budget
// (`K - max_loop_pressure`) leaves room for the while's results: the twelve
// dead-inside counts are dropped at the post-op drop site and the two
// live-through counts are reloaded after the loop. The `% 97 + 3` bound
// keeps the loop bottom-tested (no bypass edge).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    // First uses of the in-loop counts.
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(m.rotate_left(3));
    acc = acc.wrapping_sub(m.rotate_left(5));
    acc ^= m.rotate_left(7);
    acc = acc.wrapping_add(m.rotate_left(9));
    acc = acc.wrapping_sub(m.rotate_left(11));
    acc ^= m.rotate_left(13);
    acc = acc.wrapping_add(m.rotate_left(15));
    acc = acc.wrapping_sub(m.rotate_left(17));
    acc ^= m.rotate_left(19);
    acc = acc.wrapping_add(m.rotate_left(21));
    acc = acc.wrapping_sub(m.rotate_left(23));
    // First uses of the live-through counts.
    acc ^= n.rotate_left(2);
    acc = acc.wrapping_add(n.rotate_left(4));
    let iters = input2 % 97 + 3;
    let mut i: u32 = 0;
    while i < iters {
        acc ^= acc.rotate_left(1);
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7);
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13);
        acc = acc.wrapping_add(acc.rotate_left(15));
        acc = acc.wrapping_sub(acc.rotate_left(17));
        acc ^= acc.rotate_left(19);
        acc = acc.wrapping_add(acc.rotate_left(21));
        acc = acc.wrapping_sub(acc.rotate_left(23));
        if acc & 0x8000_0000_0000_0000 != 0 {
            return (acc as u32) ^ 7;
        }
        i = i.wrapping_add(1);
    }
    // Post-loop partners of the live-through counts.
    let mut r = acc;
    r ^= acc.rotate_left(2);
    r = r.wrapping_add(acc.rotate_left(4));
    (r as u32) ^ ((r >> 32) as u32)
}

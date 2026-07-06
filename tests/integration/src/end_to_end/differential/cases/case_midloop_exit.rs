// `loop { big_a; if c { break } b; }` — the exit test sits in the middle of
// the body, and `big_a` is bulky enough that LLVM loop rotation (which
// would duplicate it) is not profitable. cfg-to-scf then produces an
// scf.while whose `before` region is `big_a` + check and whose `after`
// region is non-empty (`b`), exercising the after-region emission paths of
// the scf.while lowering.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut y = input2;
    let mut i = 0u32;
    loop {
        // big_a: several mixing rounds (too big to duplicate for rotation).
        x = x.wrapping_mul(0x0808_8405).wrapping_add(y.rotate_left(7));
        y = y.wrapping_mul(0x9e37_79b9) ^ (x >> 5);
        x = x.rotate_left(13).wrapping_sub(y & 0xffff);
        y = y.rotate_right(9).wrapping_add(x | 1);
        x ^= y.wrapping_mul(31);
        i = i.wrapping_add(1);
        // mid-loop exit check
        if i >= (input2 % 89).wrapping_add(1) || (x & 0xfff) == 0x123 {
            break;
        }
        // b: post-check tail, runs only when the loop continues.
        y = y.wrapping_add(i.rotate_left(3));
        x = x.wrapping_add(y >> 11);
    }
    x ^ y ^ i
}

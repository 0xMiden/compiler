// Loop with multiple distinct exit edges (header exit, conditional break,
// early return). cfg-to-scf must multiplex the exits through a discriminator
// carried by the scf.while, exercising `transform_to_reduce_loop` and the
// post-lift scf.while/index_switch canonicalization patterns.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input2 % 211;
    let mut x = input1;
    let mut i = 0u32;
    while i < n {
        x = x.wrapping_mul(0x0100_0193) ^ i;
        // Exit 2: early return with its own computation.
        if x & 0x8000_0007 == 0x8000_0003 {
            return x.rotate_left(9).wrapping_add(input2);
        }
        // Exit 3: break with a different live-out shape.
        if x & 0x8000_000f == 0x8000_000d {
            x = x.wrapping_sub(i);
            break;
        }
        i = i.wrapping_add(1);
    }
    // Exit 1 (header) and exit 3 merge here with different `x`/`i` states.
    x ^ i.wrapping_mul(0x9e37_79b9)
}

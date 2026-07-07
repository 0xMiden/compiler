// Loop with several `continue` sites (multiple CFG backedges) plus a
// mid-body `break` (extra exit). cfg-to-scf must funnel all backedges
// through a single latch and thread the exit discriminator through paths
// that don't define it, exercising the latch multiplexing and undef/poison
// threading in `transform_to_reduce_loop`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1;
    let mut acc = 0u32;
    let mut i = 0u32;
    let n = input2 % 127;
    while i < n {
        i = i.wrapping_add(1);
        x = x.wrapping_mul(0x0101_0101) ^ i;
        if x & 3 == 0 {
            acc = acc.wrapping_add(x >> 7);
            continue; // backedge 1
        }
        if x & 0xc0 == 0x40 {
            continue; // backedge 2
        }
        if x & 0xff00 == 0x3700 {
            break; // mid-body exit
        }
        acc ^= x.rotate_left(5); // falls through to backedge 3
    }
    acc ^ x ^ i
}

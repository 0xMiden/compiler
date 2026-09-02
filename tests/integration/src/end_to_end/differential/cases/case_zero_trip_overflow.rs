// Minimal reproducer for the over-full operand stack after a zero-trip-
// capable loop: twelve shared masked rotate counts used before, inside
// (rotates of the loop-carried accumulator) and after a `while i < input2 %
// 97` loop. The spill pass spills the counts and places their reloads in
// split-edge blocks, but its SSA reconstruction never visits the split
// blocks (stale dominator tree) and erases those reloads, so the original
// counts stay live on the operand stack past their spills and the emitter
// ends up scheduling an 18-felt stack. See the `zero_trip_overflow` test.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
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
    acc ^= m.rotate_left(28);
    acc = acc.wrapping_add(n.rotate_left(30));
    let iters = input2 % 97;
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
        i = i.wrapping_add(1);
    }
    let r = acc ^ acc.rotate_left(28) ^ acc.rotate_left(30);
    (r as u32) ^ ((r >> 32) as u32)
}

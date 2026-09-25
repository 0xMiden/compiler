// Minimal reproducer for the spill-transform dominance-frontier panic: the
// `zero_trip_guard` shape with ELEVEN shared counts. The zero-trip-capable
// `while i < input2 % 97` loops keep their bypass edges; the transform splits
// six edges to place reloads and then rebuilds SSA form from a dominator tree
// computed before the splits, so a split block feeding a join with three or
// more predecessors is missing from the tree and `DominanceFrontier::new`
// unwraps `None`. See the `zero_trip_frontier` test for the full notes.
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
        i = i.wrapping_add(1);
    }
    let mut acc2 = acc | 1;
    let iters2 = input1 % 89;
    let mut j: u32 = 0;
    while j < iters2 {
        acc2 ^= acc2.rotate_left(4);
        acc2 = acc2.wrapping_add(acc2.rotate_left(6));
        j = j.wrapping_add(1);
    }
    let r = acc2 ^ acc2.rotate_left(28) ^ acc.rotate_left(30);
    (r as u32) ^ ((r >> 32) as u32)
}

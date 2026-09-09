// Twelve CSE-merged count bands whose only post-definition use sits in the
// deepest else-arm of a three-level diamond: the bands are defined in the entry
// block, the three tests in between use none of them, and the arm that consumes
// all twelve is reached only on one of the four paths. That is the shape the
// post-lift `SinkOperandDefs` is meant to move INTO the region, and the reloads
// the spill transform placed for those bands travel with it. The path is chosen
// by three bits of `input1`, and the taken path is echoed in the top nibble, so
// a grid pins each of the four exits.
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
    let mut out = acc;
    let tag;
    if (input1 >> 4) & 1 == 1 {
        if (input1 >> 5) & 1 == 1 {
            if (input1 >> 6) & 1 == 1 {
                tag = 1u32;
                out = out.wrapping_add(1);
            } else {
                tag = 2;
                out ^= acc.rotate_left(1);
                out = out.wrapping_add(out.rotate_left(3));
                out ^= acc.rotate_left(5);
                out = out.wrapping_add(out.rotate_left(7));
                out ^= acc.rotate_left(9);
                out = out.wrapping_add(out.rotate_left(11));
                out ^= acc.rotate_left(13);
                out = out.wrapping_add(out.rotate_left(15));
                out ^= acc.rotate_left(17);
                out = out.wrapping_add(out.rotate_left(19));
                out ^= acc.rotate_left(21);
                out = out.wrapping_add(out.rotate_left(23));
            }
        } else {
            tag = 3;
            out ^= acc.rotate_left(2);
        }
    } else {
        tag = 4;
        out = out.wrapping_sub(acc.rotate_left(4));
    }
    (tag << 28) | ((((out as u32) ^ ((out >> 32) as u32))) & 0x0fff_ffff)
}

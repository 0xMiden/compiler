// Passthrough-branch collapse (`SimplifyPassthroughCondBr` alternating with
// `SplitCriticalEdges`) with TWELVE CSE-merged rotate count bands crossing both
// loops. The inner loop's only exits are four in-loop `return`s plus a `break`
// placed before all of them (the producer condition: at least one return must
// follow the break), nested in a kept outer loop; each return site consumes a
// different rotation of the accumulator, so the passthrough blocks the pattern
// collapses carry live state. The bands are used before the outer loop, on the
// accumulator inside the inner loop, and after both loops, so every collapsed
// path has band traffic crossing it. Exit tag in the top nibble.
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
    let outer = (input2 % 61) + 2;
    let mut o: u32 = 0;
    while o < outer {
        let mut k: u32 = 0;
        loop {
            acc = acc.rotate_left(3) ^ (k as u64);
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
            if ((acc ^ (o as u64)) % 53) == 7 {
                break;
            }
            if (acc & 0x00ff_0000) == 0x0042_0000 {
                return (1 << 28) | ((acc as u32) & 0x0fff_ffff);
            }
            if ((acc >> 40) & 0xff) == 0x37 {
                return (2 << 28) | ((acc.rotate_left(3) as u32) & 0x0fff_ffff);
            }
            if (acc & 0x1f) == 9 && k > 2 {
                return (3 << 28) | ((acc.rotate_left(9) as u32) & 0x0fff_ffff);
            }
            if k > 30 {
                return (4 << 28) | ((acc.rotate_left(17) as u32) & 0x0fff_ffff);
            }
            k = k.wrapping_add(1);
        }
        o = o.wrapping_add(1);
    }
    let mut out = acc ^ 0x5a5a_5a5a_5a5a_5a5a;
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
    (5 << 28) | (((out as u32) ^ ((out >> 32) as u32)) & 0x0fff_ffff)
}

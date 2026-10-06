// Trap edge in a `match` ARM of the dispatch shape.
//
// `interact::dispatch_spill` verbatim — a sixteen-arm dispatch inside a kept
// loop with ten CSE-merged rotate count bands live across it, five of whose
// arms overlap the default body so `SimplifySwitchFallbackOverlap` rebuilds
// the `cf.switch` — except that arm 15 now asserts on three bits of the
// accumulator before doing its work. The trapping edge is therefore one
// `br_table` successor among sixteen, inside the region the pattern rewrites.
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
    let iters = (input2 % 13) + 2;
    let mut i: u32 = 0;
    while i < iters {
        let sel = input1.wrapping_add(i) & 15;
        match sel {
            0 => acc = acc.wrapping_add(acc.rotate_left(1) ^ 1),
            // 1, 4, 7, 10, 13 fall through to the default body.
            2 => acc = acc.wrapping_add(acc.rotate_left(5) ^ 3),
            3 => acc = acc.wrapping_add(acc.rotate_left(7) ^ 4),
            5 => acc = acc.wrapping_add(acc.rotate_left(11) ^ 6),
            6 => acc = acc.wrapping_add(acc.rotate_left(13) ^ 7),
            8 => acc = acc.wrapping_add(acc.rotate_left(17) ^ 9),
            9 => acc = acc.wrapping_add(acc.rotate_left(19) ^ 10),
            11 => acc = acc.wrapping_add(acc.rotate_left(23) ^ 12),
            12 => acc = acc.wrapping_add(acc.rotate_left(25) ^ 13),
            14 => acc = acc.wrapping_add(acc.rotate_left(29) ^ 15),
            15 => {
                assert!((acc >> 61) & 7 != 7);
                acc = acc.wrapping_add(acc.rotate_left(31) ^ 16)
            }
            _ => acc ^= acc.rotate_left(11).wrapping_mul(0x0100_0193),
        }
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
        i = i.wrapping_add(1);
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
    (out as u32) ^ ((out >> 32) as u32)
}

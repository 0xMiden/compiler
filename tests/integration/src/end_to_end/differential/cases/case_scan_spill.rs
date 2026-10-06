// Four sequential early-`break` scan loops -- the `WhileRemoveUnusedArgs`
// producer, which fires once per loop -- carrying EIGHT CSE-merged rotate count
// bands that are used before the first loop, on the accumulator inside every
// loop body, and after the last loop, so the bands cross all four loops and all
// four rewritten `scf.while`s. Each loop's break is chosen by a bit of
// `input2`, so a grid pins break-on-the-first-trip, never-break and mixed trip
// counts for every loop independently.
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
    let mut u0: u32 = 0;
    while u0 < 8 {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13) | 1;
        acc = acc.wrapping_add(acc.rotate_left(15));
        if (input2 >> (u0 & 31)) & 1 == 1 {
            break;
        }
        u0 = u0.wrapping_add(1);
    }
    let mut u1: u32 = 0;
    while u1 < 8 {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13) | 1;
        acc = acc.wrapping_add(acc.rotate_left(15));
        if (input2 >> ((u1 + 8) & 31)) & 1 == 1 {
            break;
        }
        u1 = u1.wrapping_add(1);
    }
    let mut u2: u32 = 0;
    while u2 < 8 {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13) | 1;
        acc = acc.wrapping_add(acc.rotate_left(15));
        if (input2 >> ((u2 + 16) & 31)) & 1 == 1 {
            break;
        }
        u2 = u2.wrapping_add(1);
    }
    let mut u3: u32 = 0;
    while u3 < 8 {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        acc ^= acc.rotate_left(7) | 1;
        acc = acc.wrapping_add(acc.rotate_left(9));
        acc = acc.wrapping_sub(acc.rotate_left(11));
        acc ^= acc.rotate_left(13) | 1;
        acc = acc.wrapping_add(acc.rotate_left(15));
        if (input2 >> ((u3 + 24) & 31)) & 1 == 1 {
            break;
        }
        u3 = u3.wrapping_add(1);
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
    (out as u32) ^ ((out >> 32) as u32)
}

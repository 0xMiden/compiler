// Minimal `frontier.rs:123` reproducer without a zero-trip-capable loop
// (campaign 21, B1): a sixteen-arm `match` inside a bottom-tested
// `(input1 % 13) + 2` loop, three of whose arms carry a dynamically
// impossible `panic!()` guard, with nine masked rotate count bands used
// before the loop, on the loop-carried accumulator inside it, and after it.
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
    let iters = (input1 % 13) + 2;
    let mut i: u32 = 0;
    while i < iters {
        let h = ((acc >> 5) as u32).wrapping_add(i);
        let sel = h % 16;
        match sel {
            0 => {
                acc = acc.wrapping_add(acc.rotate_left(1) ^ 1);
            }
            1 => {
                acc = acc.wrapping_add(acc.rotate_left(3) ^ 2);
            }
            2 => {
                acc = acc.wrapping_add(acc.rotate_left(5) ^ 3);
            }
            3 => {
                acc = acc.wrapping_add(acc.rotate_left(7) ^ 4);
            }
            4 => {
                if h % 10 == 4 && h % 5 == 2 {
                    panic!();
                }
                acc ^= acc.rotate_left(9);
            }
            5 => {
                acc = acc.wrapping_add(acc.rotate_left(11) ^ 6);
            }
            6 => {
                acc = acc.wrapping_add(acc.rotate_left(13) ^ 7);
            }
            7 => {
                acc = acc.wrapping_add(acc.rotate_left(15) ^ 8);
            }
            8 => {
                acc = acc.wrapping_add(acc.rotate_left(17) ^ 9);
            }
            9 => {
                if h % 6 == 5 && h % 3 == 0 {
                    panic!();
                }
                acc ^= acc.rotate_left(19);
            }
            10 => {
                acc = acc.wrapping_add(acc.rotate_left(21) ^ 11);
            }
            11 => {
                acc = acc.wrapping_add(acc.rotate_left(23) ^ 12);
            }
            12 => {
                acc = acc.wrapping_add(acc.rotate_left(25) ^ 13);
            }
            13 => {
                acc = acc.wrapping_add(acc.rotate_left(27) ^ 14);
            }
            14 => {
                if h % 14 == 9 && h % 7 == 3 {
                    panic!();
                }
                acc ^= acc.rotate_left(29);
            }
            _ => {
                acc ^= acc.rotate_left(11).wrapping_mul(0x0100_0193);
            }
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
    (out as u32) ^ ((out >> 32) as u32)
}

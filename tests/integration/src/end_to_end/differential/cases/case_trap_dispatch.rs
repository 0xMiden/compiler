// A sixteen-arm `match` on an input-derived selector where THREE arms end in
// a dynamically-impossible `panic!()` (a divergent call, not a wasm
// `unreachable`). The trapping arms leave the dispatch with successor groups
// the fallback-overlap merge can reduce all the way down to one case plus its
// fallback -- the two-successor `cf.switch` that `SimplifyCondBrLikeSwitch`
// rewrites into an equality test plus a `cf.cond_br`. Selector in the top
// nibble so a grid can pin one input per arm.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(11);
    let sel = h & 15;
    let v = match sel {
        0 => h.wrapping_mul(3) ^ input2,
        1 => h.rotate_left(7).wrapping_add(input1),
        2 => {
            // h % 10 == 4 implies h % 5 == 4, contradicting h % 5 == 2 below.
            if h % 10 == 4 && h % 5 == 2 {
                panic!();
            }
            h >> 2
        }
        3 => h.wrapping_sub(input1).rotate_right(3),
        4 => h ^ 0x5555_aaaa,
        5 => h.wrapping_add(input2),
        6 => {
            if h % 6 == 5 && h % 3 == 0 {
                panic!();
            }
            h.rotate_left(3)
        }
        7 => h.wrapping_mul(0x0100_0193),
        8 => h ^ (input1 << 3),
        9 => h.wrapping_sub(0x1234_5678),
        10 => {
            if h % 14 == 9 && h % 7 == 3 {
                panic!();
            }
            h >> 5
        }
        11 => h.rotate_right(11),
        12 => h.wrapping_add(input1 ^ input2),
        13 => h.wrapping_mul(7),
        14 => h ^ 0x0f0f_f0f0,
        _ => h.wrapping_add(1),
    };
    (sel << 28) | (v & 0x0fff_ffff)
}

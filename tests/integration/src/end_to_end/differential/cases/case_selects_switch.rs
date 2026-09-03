// select_chain x switch_forms (campaign 14): one reused condition feeds six
// selects over two multi-use u64 values that both stay live past every
// select; the selected values then drive `br_table` selectors inside a loop
// — a dense 16-arm `match` on a loop-carried selector seeded from a select,
// followed by a holey `match` with a hot default on a selector derived from
// another select and the trip counter — and every select result and both
// u64 sources are consumed after the loop.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let c = (input1 ^ input2) & 1 == 0;
    let x = ((input1 ^ 0x85eb_ca6b) as u64) | 1;
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let s0 = if c { x } else { y };
    let s1 = if c { y } else { x };
    let s2 = if c {
        x.rotate_left(3)
    } else {
        y.rotate_left(5)
    };
    let s3 = if c { y ^ 0x55 } else { x ^ 0xaa };
    let s4 = if c {
        x.wrapping_add(y)
    } else {
        x.wrapping_sub(y)
    };
    let s5 = if c {
        s0.rotate_left(1)
    } else {
        s1.rotate_left(2)
    };
    let mut sel = (s0 as u32) & 15;
    let mut acc = s4;
    let n = (input2 % 29).wrapping_add(1);
    let mut i = 0u32;
    while i < n {
        sel = match sel {
            0 => {
                acc ^= s0;
                3
            }
            1 => {
                acc = acc.wrapping_add(s1);
                7
            }
            2 => {
                acc = acc.rotate_left(2) ^ s2;
                11
            }
            3 => {
                acc = acc.wrapping_mul(3) ^ s3;
                (acc >> 60) as u32
            }
            4 => {
                acc ^= s4 >> 4;
                15
            }
            5 => {
                acc = acc.wrapping_sub(s5);
                1
            }
            6 => {
                acc ^= acc >> 6;
                13
            }
            7 => {
                acc = acc.rotate_right(7);
                9
            }
            8 => {
                acc = acc.wrapping_add(0x88);
                2
            }
            9 => {
                acc |= 9;
                0
            }
            10 => {
                acc &= 0xffff_ffff_ffff_fff0;
                12
            }
            11 => {
                acc = acc.wrapping_mul(11);
                5
            }
            12 => {
                acc = acc.rotate_left(12);
                14
            }
            13 => {
                acc = !acc;
                4
            }
            14 => {
                acc = acc.wrapping_add(x);
                8
            }
            _ => {
                acc ^= y;
                10
            }
        };
        // Holey match with a hot default on a selector from another select.
        let h = ((s1 >> (i & 31)) as u32) % 32;
        acc = match h {
            0 => acc ^ 0xa0,
            1 => acc.wrapping_add(0xa1),
            2 => acc.rotate_left(3),
            3 => acc.wrapping_sub(0xa3),
            8 => acc ^ 0xa8,
            9 => acc.wrapping_mul(9),
            10 => acc.rotate_right(5),
            11 => acc.wrapping_add(0xab),
            20 => acc ^ 0xb4,
            21 => acc.wrapping_mul(21),
            _ => acc.wrapping_add(h as u64),
        };
        i = i.wrapping_add(1);
    }
    let r = acc ^ s0 ^ s1.rotate_left(1) ^ s2.wrapping_add(s3) ^ s4.rotate_left(7) ^ s5 ^ x ^ y;
    (r as u32) ^ ((r >> 32) as u32) ^ sel
}

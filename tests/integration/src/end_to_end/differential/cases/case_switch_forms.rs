// Switch shapes: a dense 0..15 match, a holey match (cases 0-3, 8-11, 20,
// 21 with a hot default), a sparse match on a u16-derived selector, a match
// on a u8-derived selector with `|` patterns and guards, two matches on the
// SAME selector in sequence, a nested match, and a match inside a `while`
// whose selector is loop-carried (a br_table with a loop-carried index).
// Every arm computes a distinct value so an index/default mix-up changes
// the result.

// Dense, then holey with hot default, on the same selector in sequence.
#[inline(never)]
fn dense_then_holey(a: u32, b: u32) -> u32 {
    let sel = a.wrapping_mul(0x9e37_79b9) >> 27; // 0..31
    let v = match sel & 15 {
        0 => b.wrapping_add(1),
        1 => b ^ 0x11,
        2 => b.rotate_left(2),
        3 => b.wrapping_mul(3),
        4 => b >> 4,
        5 => b.wrapping_sub(5),
        6 => b ^ (b >> 6),
        7 => b.rotate_right(7),
        8 => b.wrapping_add(0x88),
        9 => b | 9,
        10 => b & 0xffff_fff0,
        11 => b.wrapping_mul(11),
        12 => b.rotate_left(12),
        13 => !b,
        14 => b.wrapping_add(a),
        _ => b ^ a,
    };
    // Second match on the same selector: holey cases, default is hot.
    match sel {
        0 => v ^ 0xa0,
        1 => v.wrapping_add(0xa1),
        2 => v.rotate_left(3),
        3 => v.wrapping_sub(0xa3),
        8 => v ^ 0xa8,
        9 => v.wrapping_mul(9),
        10 => v.rotate_right(5),
        11 => v.wrapping_add(0xab),
        20 => v ^ 0xb4,
        21 => v.wrapping_mul(21),
        _ => v.wrapping_add(sel),
    }
}

// Sparse match on a u16-derived selector.
#[inline(never)]
fn sparse_u16(a: u32, b: u32) -> u32 {
    let sel = (a ^ b.rotate_left(11)) as u16;
    match sel {
        0 => a.wrapping_add(0x1000),
        7 => a ^ 0x7000,
        100 => a.rotate_left(4),
        1000 => a.wrapping_mul(1000),
        0x7fff => a.wrapping_sub(0x7fff),
        0x8000 => a | 0x8000,
        0xffff => !a,
        _ => a ^ (sel as u32),
    }
}

// u8-derived selector with `|` patterns, ranges and guards, nested match.
#[inline(never)]
fn u8_patterns(a: u32, b: u32) -> u32 {
    let sel = (a >> 3) as u8;
    match sel {
        0 | 2 | 4 => b.wrapping_add(sel as u32),
        1 | 3 => b ^ 0x13,
        5..=9 => b.rotate_left(sel as u32),
        10..=19 if b & 1 == 1 => b.wrapping_mul(3),
        10..=19 => b.wrapping_mul(5),
        20 | 40 | 60 | 80 => match b & 3 {
            0 => b >> 1,
            1 => b.wrapping_sub(sel as u32),
            _ => b ^ (sel as u32),
        },
        100..=199 => b.wrapping_add(0x100),
        250..=255 => !b,
        _ => b,
    }
}

// br_table inside a loop with a loop-carried selector.
#[inline(never)]
fn carried_selector(a: u32, b: u32) -> u32 {
    let mut sel = a % 9;
    let mut acc = b;
    let n = (b % 53).wrapping_add(3);
    let mut i = 0u32;
    while i < n {
        sel = match sel {
            0 => {
                acc = acc.wrapping_add(i);
                3
            }
            1 => {
                acc ^= 0x101;
                if acc & 8 == 0 { 5 } else { 2 }
            }
            2 => {
                acc = acc.rotate_left(1);
                7
            }
            3 => {
                acc = acc.wrapping_mul(3);
                (acc >> 29) & 7
            }
            4 => {
                acc = acc.wrapping_sub(i);
                1
            }
            5 => {
                acc ^= i << 3;
                6
            }
            6 => {
                acc = acc.rotate_right(2);
                0
            }
            7 => {
                acc = acc.wrapping_add(0x77);
                8
            }
            _ => {
                acc ^= acc >> 5;
                4
            }
        };
        i = i.wrapping_add(1);
    }
    acc ^ sel.wrapping_mul(0x0101_0101)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    dense_then_holey(input1, input2)
        ^ sparse_u16(input2, input1).rotate_left(5)
        ^ u8_patterns(input1, input2).rotate_left(10)
        ^ carried_selector(input2, input1).rotate_left(15)
}

// An inner loop PRODUCES the selector of the outer `match` (a br_table whose
// index is a loop result), and the outer arms in turn set the inner loop's
// bound and seed — so the br_table index and the loop bound are each
// loop-carried through the other loop. Two arms exit the outer loop with
// different values; one arm returns. Exit tag = top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut bound = (input2 % 7).wrapping_add(1);
    let mut seed = input2;
    let mut outer = 0u32;
    let tag = loop {
        if outer >= 24 {
            break 1u32;
        }
        outer = outer.wrapping_add(1);
        // Inner loop: produce a selector in 0..8 from the mixing state.
        let mut j = 0u32;
        let sel = loop {
            x = x.wrapping_mul(0x9e37_79b9) ^ seed.wrapping_add(j);
            if x & 0x70 == 0x20 {
                break (x >> 8) & 7;
            }
            j = j.wrapping_add(1);
            if j >= bound {
                break 7;
            }
        };
        match sel {
            0 => bound = (x % 5).wrapping_add(1),
            1 => seed = seed.rotate_left(3),
            2 => {
                if outer > 10 {
                    break 2; // exit A with the current x
                }
                bound = 2;
            }
            3 => seed ^= x,
            4 => {
                if x & 0x3000 == 0x1000 {
                    return (4 << 28) | ((x ^ outer) & 0x0fff_ffff);
                }
                bound = (bound % 3).wrapping_add(1);
            }
            5 => x = x.rotate_left(7),
            6 => {
                if j == bound.wrapping_sub(1) {
                    break 3; // exit B: inner loop hit on its last allowed trip
                }
                seed = seed.wrapping_add(j);
            }
            _ => bound = (x % 4).wrapping_add(2),
        }
        x = x.wrapping_add(j << 4);
    };
    (tag << 28) | ((x ^ bound.rotate_left(8) ^ seed.rotate_left(16) ^ outer) & 0x0fff_ffff)
}

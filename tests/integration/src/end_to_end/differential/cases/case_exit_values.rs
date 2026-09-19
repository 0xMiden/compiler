// Multi-exit loops where every exit carries a DIFFERENT value: a labeled
// `break 'outer <value>` from inside a `match` arm of the inner loop, an
// early `return` from the inner loop, a plain inner `break`, a value-carrying
// break after the inner loop, and the outer header exit of a zero-trip-
// capable `while` (LLVM keeps its guard + bypass edge). The exit tag is the
// top nibble of the result.
#[inline(never)]
fn exits(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let n = input2 % 97; // zero-trip-capable outer bound
    let jn = (input1 % 5).wrapping_add(2); // bottom-test inner bound
    let mut i = 0u32;
    let mut j = 0u32;
    let r = 'outer: loop {
        if i >= n {
            break (1 << 28) | (x & 0x0fff_ffff); // exit H: outer header
        }
        j = 0;
        while j < jn {
            x = x.wrapping_mul(0x0100_0193) ^ i.wrapping_add(j);
            match x % 5 {
                0 => {
                    if x & 0xf0 == 0x30 {
                        break 'outer (2 << 28) | (x.wrapping_add(j) & 0x0fff_ffff); // exit L
                    }
                }
                1 => {
                    if x & 0xf00 == 0x300 {
                        return (3 << 28) | (x.wrapping_sub(i) & 0x0fff_ffff); // exit R
                    }
                }
                2 => {
                    if x & 0xf000 == 0x3000 {
                        break; // inner break, falls to the post-inner check
                    }
                }
                3 => x = x.rotate_left(3),
                _ => {}
            }
            j = j.wrapping_add(1);
        }
        if x & 0x7_0000 == 0x3_0000 {
            break (4 << 28) | (j.wrapping_mul(0x0101_0101) & 0x0fff_ffff); // exit P
        }
        i = i.wrapping_add(1);
    };
    r ^ (i.wrapping_mul(0x9e37_79b9) & 0x0fff_ffff)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    exits(input1, input2)
}

// Region nesting that stresses exit-block reduction: a diamond inside a
// loop inside a diamond inside a loop, where each diamond arm contains its
// own small loop or a break, every join carries a different set of live
// values, and the inner loop exits through three sites (two of them inside
// diamond arms). The outer loop bound is zero-trip-capable. Exit tag = top
// nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut y = input2;
    let n = input2 % 41; // zero-trip-capable
    let mut i = 0u32;
    let mut tag = 1u32;
    'outer: while i < n {
        // Outer diamond on an input-derived condition.
        if (x ^ i) & 4 == 0 {
            // Then arm: a loop with an inner diamond and two arm exits.
            let mut j = 0u32;
            let jn = (x % 7).wrapping_add(1);
            while j < jn {
                y = y.wrapping_mul(0x9e37_79b9) ^ j;
                if y & 0x30 == 0x10 {
                    // Inner diamond then-arm: small loop then maybe break.
                    let mut k = 0u32;
                    while k < (y % 3).wrapping_add(1) {
                        x = x.rotate_left(1).wrapping_add(k);
                        k = k.wrapping_add(1);
                    }
                    if x & 0x700 == 0x300 {
                        tag = 2;
                        break 'outer;
                    }
                } else {
                    // Inner diamond else-arm: break out of the middle loop.
                    x ^= y >> 3;
                    if x & 0x7000 == 0x3000 {
                        tag = 3;
                        y = y.wrapping_add(j);
                        break;
                    }
                }
                j = j.wrapping_add(1);
            }
            x = x.wrapping_add(j << 4);
        } else {
            // Else arm: a different small loop, then a possible early return.
            let mut k = 0u32;
            while k < (i % 4).wrapping_add(1) {
                y = y.rotate_right(3) ^ x;
                k = k.wrapping_add(1);
            }
            if y & 0xf_0000 == 0x3_0000 {
                return (4 << 28) | ((x ^ y ^ i) & 0x0fff_ffff);
            }
            x = x.wrapping_sub(k);
        }
        // Join of the outer diamond: both arms fall through here.
        i = i.wrapping_add(1);
        x = x.wrapping_add(y & 0xff);
    }
    (tag << 28) | ((x ^ y.rotate_left(7) ^ i.wrapping_mul(0x0101_0101)) & 0x0fff_ffff)
}

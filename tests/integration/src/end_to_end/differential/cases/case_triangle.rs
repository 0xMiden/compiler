// Triangle loop nests: `for i in 0..n { for j in 0..i { .. } }` where the
// inner loop is zero-trip at i == 0 and its bound is the outer counter, a
// third level bounded by `j`, an early return from the innermost body, a
// labeled `continue 'outer` from the middle level, and a `break` of the
// inner loop that leaves `j` partially advanced for the outer tail. Exit
// tag = top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input2 % 19; // zero-trip-capable outer bound
    let mut x = input1 | 1;
    let mut tag = 1u32;
    let mut visits = 0u32;
    let mut i = 0u32;
    'outer: while i < n {
        let mut j = 0u32;
        while j < i {
            let mut k = 0u32;
            while k < j {
                visits = visits.wrapping_add(1);
                x = x.wrapping_mul(0x0100_0193) ^ (i << 8 | j << 4 | k);
                if x & 0x3ff == 0x155 {
                    return (4 << 28) | ((x ^ visits) & 0x0fff_ffff);
                }
                k = k.wrapping_add(1);
            }
            x = x.wrapping_add(k);
            if x & 0xff == 0x77 {
                tag = 2;
                x = x.rotate_left(5);
                i = i.wrapping_add(1);
                continue 'outer;
            }
            if x & 0x1ff == 0x0aa {
                tag = 3;
                x ^= j << 12;
                break;
            }
            j = j.wrapping_add(1);
        }
        x = x.wrapping_add(j << 16);
        i = i.wrapping_add(1);
    }
    (tag << 28) | ((x ^ visits.wrapping_mul(0x9e37_79b9) ^ i.rotate_left(20)) & 0x0fff_ffff)
}

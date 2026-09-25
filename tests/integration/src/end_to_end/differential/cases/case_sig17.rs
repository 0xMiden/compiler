// A helper whose argument list is seventeen felts — one u32 and eight u64s
// passed BY VALUE — called from a loop. Ordinary Rust (a round function over
// an eight-word state), but one felt over the 16-felt call-signature limit.
#[inline(never)]
fn round(i: u32, s0: u64, s1: u64, s2: u64, s3: u64, s4: u64, s5: u64, s6: u64, s7: u64) -> u64 {
    let k = i as u64;
    (s0 ^ k.rotate_left(7))
        .wrapping_add(s1 ^ s2.rotate_left(13))
        .wrapping_mul(s3 | 1)
        .wrapping_sub(s4 ^ s5.rotate_left(29))
        ^ s6.wrapping_add(s7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut s = [0u64; 8];
    let mut i = 0usize;
    while i < 8 {
        s[i] = ((input1 as u64) << 32 | input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15 + i as u64);
        i += 1;
    }
    let mut acc = 0u64;
    let mut n = 0u32;
    while n < (input2 % 5) + 1 {
        acc ^= round(n, s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]);
        s[(n as usize) & 7] = acc;
        n += 1;
    }
    (acc as u32) ^ ((acc >> 32) as u32)
}

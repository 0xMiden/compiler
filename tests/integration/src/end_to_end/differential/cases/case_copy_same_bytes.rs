// Passing sibling of `copy_same_pos`: the same identical-range `copy_within`
// (runtime shift that can be 0) on a BYTE buffer with an odd length and an
// odd start, so the byte count is never a multiple of 4 and the MASM memcpy
// lowering takes its byte fallback loop instead of `memcopy_elements`. The
// loop copies each byte onto itself, which is harmless, so the identical
// range agrees with the native no-op memmove; the odd shift selects a
// disjoint destination 20 bytes away.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = [0u8; 48];
    let mut i = 0u32;
    while i < 48 {
        b[i as usize] = (input1.wrapping_mul(i + 1) ^ (input2 >> (i & 7))) as u8;
        i += 1;
    }
    let n = (((input2 % 4) * 2) + 1) as usize; // 1, 3, 5, 7 bytes
    let src = ((input1 % 4) * 2 + 1) as usize; // 1, 3, 5, 7
    let shift = ((input1 >> 2) & 1) as usize; // 0 = same range, 1 = disjoint
    let dst = src + 20 * shift;
    b.copy_within(src..src + n, dst);
    let mut acc = 0u32;
    let mut j = 0usize;
    while j < 48 {
        acc = acc.rotate_left(3) ^ (b[j] as u32).wrapping_mul(j as u32 | 1);
        j += 1;
    }
    acc
}

// Campaign 30 / W4: FORWARD-overlapping `copy_within` (destination strictly
// below the source) at every overlap distance 1..=8, with an odd length so
// `count % 4 != 0` always and the memcpy lowering always takes its byte
// fallback loop. That loop copies element 0 upward, which is exactly the
// direction a `dst < src` memmove needs, so these ranges must agree with
// native even though they overlap. (The other direction, `dst > src`, is the
// known `memory::mem_overlap` bug, and an overlapping 4-aligned range is the
// `memcopy_elements` overlap assert — neither is retried here.) A disjoint
// 4-aligned copy in the same case keeps the element fast path covered.
#[repr(C, align(4))]
struct Buf([u8; 96]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0u8; 96]);
    let mut k = 0usize;
    while k < 96 {
        b.0[k] = (input1.rotate_left(k as u32 & 31) ^ input2.wrapping_mul(k as u32 + 17)) as u8;
        k += 1;
    }
    let d = ((input1 >> 3) % 8) as usize + 1; // overlap distance 1..=8
    let len = 2 * ((input2 >> 3) % 8) as usize + 1; // odd length 1..=15
    let src = 32 + (input2 % 7) as usize; // 32..=38
    let dst = src - d; // strictly below the source
    let mut acc = 0u32;

    // Forward overlap: the ranges share `len - d` bytes whenever len > d.
    b.0.copy_within(src..src + len, dst);
    acc = acc.rotate_left(3) ^ (d as u32) ^ ((len as u32) << 8) ^ ((src as u32) << 16);

    // A second forward overlap two bytes wider, so the shared prefix grows.
    // The length stays ODD: an overlapping range whose byte count IS a
    // multiple of four takes the element fast path, whose `memcopy_elements`
    // overlap assert rejects overlap in BOTH directions (see `copy_fwd`'s
    // doc comment).
    let src2 = 64 + (input1 % 5) as usize;
    b.0.copy_within(src2..src2 + len + 2, src2 - d);

    // Disjoint 4-aligned copy in the same case: the element fast path.
    let words = ((input2 >> 6) % 5) as usize; // 0..=4 words
    b.0.copy_within(0..words * 4, 16);

    // Reads at the seam of the first copy.
    let p0 = b.0[dst] as u32;
    let p1 = b.0[dst + len - 1] as u32;
    let p2 = b.0[src] as u32;
    acc = acc.rotate_left(5) ^ p0.wrapping_mul(0x0100_0193) ^ p1 ^ p2.wrapping_mul(7);

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 96 {
        s = s.rotate_left(3).wrapping_add(b.0[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

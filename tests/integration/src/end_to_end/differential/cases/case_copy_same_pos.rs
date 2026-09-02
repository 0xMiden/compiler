// `copy_within` with a runtime destination that can coincide EXACTLY with
// its source (shift 0) — a no-op memmove natively. The u32 element ranges
// keep every byte address and byte count 4-aligned, so the MASM memcpy
// lowering takes its element fast path and hands the copy to miden-core-lib
// `memcopy_elements`, whose overlap assert (`wp >= rp + n || rp >= wp + n`)
// rejects wp == rp with n > 0. Odd `input1 >> 2` selects a disjoint
// destination 16 elements away (the passing sibling shape); even selects
// the identical range. No two ranges ever partially overlap, so this is
// not the `mem_overlap` (dst > src) shape.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 32];
    let mut i = 0u32;
    while i < 32 {
        a[i as usize] = input1.wrapping_mul(i + 1) ^ input2.rotate_left(i);
        i += 1;
    }
    let n = ((input2 % 4) + 1) as usize; // 1..=4 elements = 4..=16 bytes
    let src = ((input1 % 3) * 4) as usize; // 0, 4, 8
    let shift = ((input1 >> 2) & 1) as usize; // 0 = same range, 1 = disjoint
    let dst = src + 16 * shift;
    a.copy_within(src..src + n, dst);
    let mut acc = 0u32;
    let mut j = 0usize;
    while j < 32 {
        acc = acc.rotate_left(3) ^ a[j].wrapping_mul(j as u32 | 1);
        j += 1;
    }
    acc
}

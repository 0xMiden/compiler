// Edge-count logical shifts and rotates on u32/u64: wrapping_shl/wrapping_shr/
// rotate_left/rotate_right with a fully dynamic count. Rust masks the count
// (`count % width`); asserts the VM lowering masks identically at the
// boundaries. Edge relations asserted by the pinned grid: count 0 (identity),
// width-1, width (masks to 0), width+1, 2*width, and over-2*width counts
// (67/96/131), on values 0, 1, 0x7FFFFFFF, 0x80000001, and u32::MAX.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v32 = input1;
    let v64: u64 = ((input1 as u64) << 32) | (input1 ^ 0x9e37_79b9) as u64;
    let c = input2;

    let a = v32.wrapping_shl(c);
    let b = v32.wrapping_shr(c);
    let r1 = v32.rotate_left(c);
    let r2 = v32.rotate_right(c);

    let d = v64.wrapping_shl(c);
    let e = v64.wrapping_shr(c);
    let r3 = v64.rotate_left(c);
    let r4 = v64.rotate_right(c);

    let m = d ^ e.rotate_left(3) ^ r3.rotate_left(7) ^ r4.rotate_left(11);
    a.wrapping_add(b.rotate_left(5))
        .wrapping_add(r1.rotate_left(9))
        .wrapping_add(r2.rotate_left(13))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

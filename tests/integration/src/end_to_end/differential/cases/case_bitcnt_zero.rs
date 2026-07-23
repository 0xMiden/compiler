// Bit-count saturation edges: leading_zeros/trailing_zeros/count_ones of
// EXACTLY 0 (clz(0) == width, ctz(0) == width — the edge arms of the VM-side
// intrinsics) and of MAX, on u32, u64, and u128 (both limbs zero at once).
// Edge relations asserted by the pinned grid: the all-zero row (0, 0), the
// all-ones row (MAX, MAX), and limb-boundary counts — ctz(1<<32) == 32,
// clz(u64 of 0x80000000) == 32, plus single-bit values at both ends.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v = input1;
    let w: u64 = ((input1 as u64) << 32) | input2 as u64;
    let x: u128 = ((w as u128) << 64) | (((input2 as u64) << 32) | input1 as u64) as u128;

    let a = v.leading_zeros(); // clz(0) == 32
    let b = v.trailing_zeros(); // ctz(0) == 32
    let c = v.count_ones();
    let d = w.leading_zeros(); // clz(0) == 64
    let e = w.trailing_zeros(); // ctz(0) == 64
    let f = w.count_ones();
    let g = x.leading_zeros(); // clz(0) == 128 (both limbs zero)
    let h = x.trailing_zeros(); // ctz(0) == 128
    let i = x.count_ones();

    a.wrapping_add(b.rotate_left(3))
        .wrapping_add(c.rotate_left(6))
        .wrapping_add(d.rotate_left(9))
        .wrapping_add(e.rotate_left(12))
        .wrapping_add(f.rotate_left(15))
        .wrapping_add(g.rotate_left(18))
        .wrapping_add(h.rotate_left(21))
        .wrapping_add(i.rotate_left(24))
}

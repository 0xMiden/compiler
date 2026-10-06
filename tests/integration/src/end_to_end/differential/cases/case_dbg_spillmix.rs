// Debug decorators under spill pressure: shared masked shift-count bands (the
// only plain-Rust cross-block W traffic) crossing a pressure-asymmetric
// diamond while NAMED u64 locals carry values through it. The spill
// transform's edge splits/reloads now run interleaved with DebugVar + Nop
// lowering — debug info must never change scheduling semantics. Freight is
// kept around six felts, clear of the known ten-band `rotl_window` scheduler
// panic (see the ignored reproducers in the spills module).

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let wide: u64 = ((input1 as u64) << 32) | input2 as u64;
    let band_a = input2 & 63;
    let band_b = (input2 >> 6) & 63;
    let band_c = (input2 >> 12) & 63;
    let mut carry = wide | 1;
    if input1 & 1 == 0 {
        let heavy = carry.rotate_left(band_a).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let lean = heavy.rotate_right(band_b) ^ wide;
        carry = lean.wrapping_add(heavy >> (band_c | 1));
    } else {
        carry = carry.wrapping_add(0x0102_0304);
    }
    let tail = carry.rotate_left(band_a) ^ carry.rotate_right(band_b);
    let fold = tail.wrapping_add(carry >> band_c);
    (fold as u32) ^ ((fold >> 32) as u32)
}

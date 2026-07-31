// Arithmetic shift right at sign/count boundaries: i32/i64 wrapping_shr with a
// fully dynamic (unmasked) count -> `::intrinsics::i32/i64::checked_shr`, plus
// the constant sign-mask idiom `>> 31` / `>> 63`. Edge relations asserted by
// the pinned grid: MIN >> 0 == MIN, MIN >> width-1 == -1, count == width masks
// to 0 (identity on MIN), over-width counts mask (67 -> 3), and -1 >> c == -1.
// The grid row (0x80000000, 0) makes the i64 operand exactly i64::MIN.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v = input1 as i32;
    let w = (((input1 as u64) << 32) | input2 as u64) as i64;
    let c = input2;

    let s1 = v.wrapping_shr(c); // dynamic i32.shr_s, count masked % 32
    let s2 = w.wrapping_shr(c); // dynamic i64.shr_s, count masked % 64
    let s3 = v >> 31; // constant sign-mask i32
    let s4 = w >> 63; // constant sign-mask i64

    let m = (s2 as u64) ^ (s4 as u64).rotate_left(9);
    (s1 as u32)
        .wrapping_add((s3 as u32).rotate_left(5))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

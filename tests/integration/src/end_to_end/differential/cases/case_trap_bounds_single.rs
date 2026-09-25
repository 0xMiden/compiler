// Trap parity at the sharpest possible bounds check: the array length is the
// index modulus minus one, so exactly ONE of the sixteen index values (15) is
// out of range. A guard that is folded away, widened by one, or compiled as
// `<=` instead of `<` shows up here and nowhere else — every other index must
// return the element.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let t: [u32; 15] = [
        0x0000_0001,
        0x0000_0002,
        0x0000_0004,
        0x0000_0008,
        0x0000_0010,
        0x0000_0020,
        0x0000_0040,
        0x0000_0080,
        0x0000_0100,
        0x0000_0200,
        0x0000_0400,
        0x0000_0800,
        0x0000_1000,
        0x0000_2000,
        0x0000_4000,
    ];
    let i = (input1 % 16) as usize;
    t[i].wrapping_mul(input2 | 1) ^ (i as u32)
}

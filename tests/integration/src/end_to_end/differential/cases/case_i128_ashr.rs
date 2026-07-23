// Dynamic i128 ARITHMETIC shift right with counts masked to [0, 128): the
// count >= 64 leg fills the high limb from the sign (i64.shr_s by 63) — sign
// propagation across limbs the corpus never executed. Two dynamic-sign values
// (limbs swapped) with independently derived counts, so both signs and both
// count ranges occur across the 16 random inputs.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = (input1 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ input2 as u64;
    let w1: i128 = (((a as u128) << 64) | b as u128) as i128; // sign from a's top bit
    let w2: i128 = (((b as u128) << 64) | a as u128) as i128; // sign from b's top bit

    let r1 = w1 >> (input2 & 127);
    let r2 = w2 >> ((input1 >> 3) & 127);

    let m = (r1 as u64) ^ ((r1 >> 64) as u64) ^ (r2 as u64) ^ ((r2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}

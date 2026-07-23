// Dynamic u128 logical shifts with counts masked to [0, 128): LLVM legalizes
// 128-bit shl/shr into two-limb funnel/select chains branching on count < 64
// vs >= 64 — never executed by the corpus (u128_mix only has a constant
// rotate). Random inputs land the counts on both sides of 64; left and right
// shifts use independently derived counts.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = (input1 as u64).wrapping_mul(0x0101_0101_0101_0101);
    let b: u64 = ((input2 as u64) << 32) | input1 as u64;
    let v: u128 = ((a as u128) << 64) | b as u128;

    let s1 = v << (input2 & 127);
    let s2 = v >> ((input1 ^ input2) & 127);

    let m = (s1 as u64) ^ ((s1 >> 64) as u64) ^ (s2 as u64) ^ ((s2 >> 64) as u64);
    (m as u32) ^ ((m >> 32) as u32)
}

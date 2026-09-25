// Trap parity: 64-bit division and remainder, which go through the
// `u64::div` / `i64` intrinsics on Miden rather than a native instruction.
// The divisors come from `input2 % 5`: the unsigned one is zero at residue
// 0 and the signed one (shifted down by two) is zero at residue 2 and -1 at
// residue 1. The dividend's low word is picked from {0, 1, u32::MAX,
// i32::MAX} by `input2 % 4`, so `i64::MIN / -1` and its non-overflowing
// neighbours (MIN+1, MAX, 0, MIN/1) are all reachable with
// `input1 = 0x8000_0000` and the right residue pair.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let lo: u32 = match input2 % 4 {
        0 => 0,
        1 => 1,
        2 => 0xffff_ffff,
        _ => 0x7fff_ffff,
    };
    let wide = ((input1 as u64) << 32) | (lo as u64);
    let du = (input2 % 5) as u64;
    let qu = wide / du;
    let ru = wide % du;
    let sw = wide as i64;
    let sd = ((input2 % 5) as i32 - 2) as i64;
    let qs = sw / sd;
    let rs = sw % sd;
    (qu as u32) ^ ((ru >> 32) as u32) ^ (qs as u32) ^ ((rs >> 16) as u32)
}

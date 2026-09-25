// Bound for the `core_parse_i64` divergence (campaign 27): the signed
// widening multiply on its own. `(a as i128) * (b as i128)` over two
// runtime `i64` operands is the plain-Rust producer of `i64.mul_wide_s`
// (both halves used), and it agrees with native — so the divergence in the
// signed `from_str` accumulation is not the wide multiply by itself.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32 as i64;
    let b = input2 as i32 as i64;
    let p = (a as i128) * (b as i128);
    ((p >> 64) as u32) ^ (p as u32)
}

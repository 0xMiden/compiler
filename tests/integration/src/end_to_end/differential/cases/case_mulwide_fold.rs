// `i64.mul_wide_s` with one POSITIVE constant multiplicand: the translation
// sign-extends the constant to i128, and `Sext::fold` materializes an I128
// immediate (the constant must be positive as an i64 —
// `materialize_constant` coerces via `as_u64`, rejecting negatives), which
// the MASM scheduler then pushes via `push_i128`
// (`codegen/masm/src/emit/int128.rs`) — its only Rust-reachable producer.
// The second operand stays dynamic so the multiply itself survives.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: i64 = (((input1 as u64) << 32) | input2 as u64) as i64;

    let w: i128 = (a as i128).wrapping_mul(0x1CED_C0FF_EE15_600D_i64 as i128);
    let hi = (w >> 64) as u64;
    let lo = w as u64;

    let m = hi ^ lo;
    (m as u32) ^ ((m >> 32) as u32)
}

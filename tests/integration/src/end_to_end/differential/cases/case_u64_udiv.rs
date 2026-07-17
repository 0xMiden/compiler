// Exercises unsigned u64 division/remainder (`i64.div_u`/`i64.rem_u` with
// dynamic guarded non-zero divisors), reaching `checked_div_u64` and
// `checked_mod_u64` in `codegen/masm/src/emit/int64.rs`, which exec the
// miden-core-lib `::miden::core::math::u64::{div,mod}` procedures.
//
// KNOWN FAILURE: the VM aborts at runtime inside `u64::div` when it hits
// `emit.U64_DIV_EVENT` (miden-core-lib asm/math/u64.masm:372): "error during
// processing of event with ID: 14153021663962350784". Any u64 division
// executed by the differential VM executor aborts this way.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = ((input2 as u64) << 21) ^ (input1 as u64).wrapping_mul(0x9E37_79B9);

    // u64 unsigned division/remainder with dynamic (guarded non-zero) divisors.
    let q = a / (b | 1);
    let r = b % (a | 1);

    let m = q ^ r;
    (m as u32) ^ ((m >> 32) as u32)
}

// KNOWN FAILURE (guest toolchain, not the Miden compiler): `i64::checked_pow`
// with a dynamic exponent. Its square-and-multiply loop calls `checked_mul`
// twice per iteration, and LLVM (rustc 1.97.0-nightly c935696dd / LLVM
// 22.1.4, `+wide-arithmetic` as set by cargo-miden) emits each overflow test
// `hi != (lo >> 63)` as `i64.mul_wide_s` with the `local.get` of the hi
// result placed BEFORE the multiply that defines it, so every test reads the
// PREVIOUS iteration's hi word (zero on the first). The VM executes that
// wasm faithfully (wasmtime agrees with the MASM result); the native build
// is right. Straight-line `checked_mul`/`overflowing_mul` (ovf_mul) and
// u64 `checked_pow` (int_logs) are stackified in a valid order and pass.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let w = (((input1 as u64) << 32) | input2 as u64) as i64;
    let e = input2 & 7;
    let m = w.checked_pow(e).unwrap_or(0x1CED_C0FF_EE15_600D) as u64;
    (m as u32) ^ ((m >> 32) as u32)
}

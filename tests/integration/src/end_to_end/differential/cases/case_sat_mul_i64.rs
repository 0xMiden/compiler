// KNOWN FAILURE (guest toolchain, not the Miden compiler): `i64::saturating_mul`
// on dynamic operands. Rust lowers it to `checked_mul` + a sign select, and
// LLVM (rustc 1.97.0-nightly c935696dd / LLVM 22.1.4, `+wide-arithmetic` as
// set by cargo-miden) emits `i64.mul_wide_s` for the overflow test
// `hi == (lo >> 63)` with the `local.get` of the hi result placed BEFORE the
// `i64.mul_wide_s` that defines it (RegStackify sinks the multiply into the
// second operand's subtree of the compare that feeds `br_if`), so the wasm
// compares a stale zero local: the product is returned unsaturated whenever
// it overflows to a non-negative low word, and saturated when it does not
// overflow but has a negative low word. The VM executes that wasm faithfully
// (wasmtime agrees with the MASM result); the native build is right.
// Two forms: distinct operands (a << 32) * (b << 32 | b), and the x * x form
// (b << 32 | b) >> 1 squared, which is i64::MAX * i64::MAX at input2 == MAX.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) as i64;
    let b = (((input2 as u64) << 32) | input2 as u64) as i64;
    let x = (b as u64 >> 1) as i64;
    let m = (a.saturating_mul(b) as u64) ^ (x.saturating_mul(x) as u64).rotate_left(1);
    (m as u32) ^ ((m >> 32) as u32)
}

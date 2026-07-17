// Dynamic signed widening multiply: `(a as i128).wrapping_mul(b as i128)`
// with BOTH operands runtime i64 values emits `i64.mul_wide_s`, whose
// translation sign-extends both operands to i128 (`arith.sext` I64 -> I128),
// reaching `sext_int64(128)` in `codegen/masm/src/emit/int64.rs` — the only
// Rust-reachable producer of a 64->128-bit sign extension. The unsigned twin
// lives in zext_wide_ctz; the constant-multiplicand fold variant is the
// (ignored) sext_shapes divergence, so no constants here.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: i64 = (((input1 as u64) << 32) | input2 as u64) as i64;
    let b: i64 = (((input2 as u64) << 17) ^ (input1 as u64).wrapping_mul(0x0101_0193)) as i64;

    let w: i128 = (a as i128).wrapping_mul(b as i128);
    let hi = (w >> 64) as u64;
    let lo = w as u64;

    let m = hi ^ lo;
    (m as u32) ^ ((m >> 32) as u32)
}

// Exercises signed i64 division: `checked_div`'s `Type::I64` arm ->
// `checked_div_i64` in `codegen/masm/src/emit/int64.rs` (exec
// `::intrinsics::i64::checked_div`). Divisors are dynamic with range-limited
// magnitude plus one (never zero), and the numerator paired with the negative
// divisor is forced odd (`| 1`, never `i64::MIN`), so `i64::MIN / -1` and
// divide-by-zero are impossible by construction and LLVM emits bare
// `i64.div_s`. i64 `%` is deliberately absent: `arith.Mod` on I64 has no
// emitter arm (compile-time `unimplemented!`).
//
// KNOWN FAILURE (expected): `::intrinsics::i64::checked_div` execs the same
// miden-core-lib `::miden::core::math::u64::div` whose `emit.U64_DIV_EVENT`
// aborts the differential executor (see u64_udiv).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = (((input1 as u64) << 32) | input2 as u64) as i64;
    let dp = ((input2 % 100_000) as i64) + 1; // dynamic divisor in [1, 100000]
    let dn = -(((input1 % 99_991) as i64) + 1); // dynamic divisor in [-99991, -1]

    let q1 = n / dp; // both numerator signs / positive divisor
    let q2 = (n | 1) / dn; // odd (never-MIN) numerator / negative divisor

    let m = (q1 as u64) ^ (q2 as u64).rotate_left(13);
    (m as u32) ^ ((m >> 32) as u32)
}

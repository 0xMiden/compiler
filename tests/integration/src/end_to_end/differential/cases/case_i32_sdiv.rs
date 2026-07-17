// Exercises signed i32 division/remainder: `checked_div`'s `Type::I32` arm ->
// `checked_div_i32` (exec `::intrinsics::i32::checked_div`) and
// `wasm.I32RemS` -> `wrapping_mod` -> `wrapping_mod_i32` (exec
// `::intrinsics::i32::wrapping_mod`), covering all four sign combinations and
// the truncate-toward-zero remainder-sign semantics. Native panics are
// impossible by construction: divisors have range-limited magnitude plus one
// (never zero, and the positive divisor excludes MIN/-1), and numerators
// paired with the negative divisor are forced odd (`| 1`, never `i32::MIN`),
// so LLVM emits bare `i32.div_s`/`i32.rem_s`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input1 as i32; // numerator, both signs
    let m = (input2 ^ 0xDEAD_BEEF) as i32; // second numerator, both signs
    let dp = ((input2 % 1000) as i32) + 1; // dynamic divisor in [1, 1000]
    let dn = -((((input1 >> 7) % 997) as i32) + 1); // dynamic divisor in [-997, -1]

    // Division: both numerator signs by a positive divisor, and an odd
    // (never-MIN) numerator by a negative divisor.
    let q1 = n / dp;
    let q2 = (m | 1) / dn;

    // Remainder: all four sign combinations (odd numerators for `dn`).
    let r1 = n % dp;
    let r2 = (m | 1) % dn;
    let r3 = (n | 1) % dn;
    let r4 = m % dp;

    (q1 as u32)
        .wrapping_add((q2 as u32).rotate_left(5))
        .wrapping_add((r1 as u32).rotate_left(11))
        .wrapping_add((r2 as u32).rotate_left(17))
        .wrapping_add((r3 as u32).rotate_left(23))
        .wrapping_add((r4 as u32).rotate_left(29))
}

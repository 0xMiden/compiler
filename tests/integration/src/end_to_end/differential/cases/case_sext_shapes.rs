// Exercises signed widening shapes, which the corpus otherwise lacks entirely
// (no other case creates an `arith.sext`): `i64.extend_i32_s`,
// `i64.extend8_s`/`extend16_s`/`extend32_s`, `i32.extend8_s`/`extend16_s`, and
// `i64.mul_wide_s`. The wide multiply sign-extends *both* operands to i128 at
// translation time, so a constant multiplicand becomes
// `arith.sext(arith.constant : i64) : i128`, which the canonicalizer folds
// (`Sext::fold` I128 arm + constant materialization).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32;
    let b = input2 as i32;

    // i64.extend_i32_s -> arith.sext : i32 -> i64
    let wa = a as i64;
    let wb = b as i64;

    // i64.mul_wide_s with a constant multiplicand (folds to a constant sext).
    let wide = (wa as i128).wrapping_mul(-0x1234_5678_9ABC_DEF1_i64 as i128);
    let hi = (wide >> 64) as i64;
    let lo = wide as i64;

    // i64.mul_wide_s with dynamic operands.
    let wide2 = (wa as i128).wrapping_mul(wb as i128);
    let hi2 = (wide2 >> 64) as i64;
    let lo2 = wide2 as i64;

    // Narrow re-extensions: i64.extend8_s / i64.extend16_s / i64.extend32_s
    // and i32.extend8_s / i32.extend16_s.
    let n8 = (lo as i8) as i64;
    let n16 = (hi as i16) as i64;
    let n32 = (lo2 as i32) as i64;
    let m8 = (b as i8) as i32;
    let m16 = (a as i16) as i32;

    let acc = hi ^ lo ^ hi2 ^ n8 ^ n16 ^ n32;
    (acc as u32) ^ ((acc >> 32) as u32) ^ (m8 as u32) ^ (m16 as u32)
}

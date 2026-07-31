// u128 `/` and `%` boundary RELATIONS the u128_udiv/u128_umod derivations
// cannot reach: divisor exactly == dividend (n | 1 with n odd), divisor ==
// dividend + 1 (n even), both-limbs-max operands (inputs (MAX, MAX) make the
// value u128::MAX), and divisor exactly 1 with a nonzero dividend. The `/`
// and `%` operand pairs are DIFFERENT values (limbs swapped) so LLVM cannot
// strength-reduce a same-pair div+rem to mul-sub (KNOWLEDGE.md) — both
// __udivti3 and __umodti3 execute on every input.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let lo: u64 = ((input2 as u64) << 32) | input1 as u64;
    let hi: u64 = ((input1 as u64) << 32) | input2 as u64;
    let n: u128 = ((hi as u128) << 64) | lo as u128;
    // Limb-swapped twin for the remainder pairs.
    let m: u128 = ((lo as u128) << 64) | hi as u128;

    // n odd: divisor == dividend exactly (q1 == 1); n even: divisor == n + 1,
    // the smallest divisor > dividend (q1 == 0).
    let q1 = n / (n | 1);
    // input2 == 0 makes the divisor exactly 1 (q2 == n).
    let q2 = n / (((input2 as u64) as u128) | 1);

    // Same relations against __umodti3, on the swapped-limb value.
    let r1 = m % (m | 1);
    // input1 == 0 makes the divisor exactly 1 (r2 == 0).
    let r2 = m % (((input1 as u64) as u128) | 1);

    let x = q1.wrapping_add(q2) ^ r1.wrapping_mul(3).wrapping_add(r2);
    let f = (x as u64) ^ ((x >> 64) as u64);
    (f as u32) ^ ((f >> 32) as u32)
}

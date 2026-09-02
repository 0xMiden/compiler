// 128-bit division/remainder GUARD shapes with dynamic divisors that reach
// 0, -1 and MIN: i128 checked_div / checked_rem / wrapping_div /
// wrapping_rem / overflowing_div and u128 checked_div / checked_rem. LLVM
// wraps the compiler-builtins `__divti3`/`__modti3`/`__udivti3`/`__umodti3`
// calls in `rhs == 0` and `rhs == -1 && lhs == MIN` branches, so the i128
// MIN / -1 row must take the guard arms (None / wrapping MIN / rem 0 /
// overflow flag) without calling the builtin, while every other row runs
// the builtin on the VM with a full-width dividend. Dividend = (input1 << 96
// | !input2 << 64 | input1 << 32 | input2) (i128::MIN at (0x80000000,
// 0xFFFFFFFF)), signed divisor = sign-extended input2 (0 / -1 / i32::MIN),
// unsigned divisor = input2 splat. Remainders use a rotated dividend so no
// `%` shares an operand pair with a `/`. Lives in `#[inline(never)]`
// helpers to keep the entrypoint's live pressure low.
#[inline(never)]
fn fold128(p: u128) -> u64 {
    (p as u64) ^ ((p >> 64) as u64).rotate_left(29)
}

#[inline(never)]
fn sdiv(n: i128, d: i128) -> u64 {
    let nr = n.rotate_left(40);
    let mut m = fold128(n.checked_div(d).map_or(0x1111_1111_1111_1111_1111_1111_1111_1111, |q| q as u128));
    m ^= fold128(nr.checked_rem(d).map_or(0x2222_2222_2222_2222_2222_2222_2222_2222, |r| r as u128))
        .rotate_left(3);
    if d != 0 {
        m ^= fold128(n.wrapping_neg().wrapping_div(d) as u128).rotate_left(7);
        m ^= fold128(nr.wrapping_rem(d) as u128).rotate_left(11);
        let (q, o) = n.overflowing_div(d);
        m ^= fold128(q as u128).rotate_left(13) ^ (o as u64);
    }
    m
}

#[inline(never)]
fn udiv(n: u128, d: u128) -> u64 {
    let nr = n.rotate_left(40);
    let mut m = fold128(n.checked_div(d).unwrap_or(0x3333_3333_3333_3333_3333_3333_3333_3333));
    m ^= fold128(nr.checked_rem(d).unwrap_or(0x4444_4444_4444_4444_4444_4444_4444_4444)).rotate_left(5);
    m
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n: u128 = ((input1 as u128) << 96)
        | (((!input2) as u128) << 64)
        | ((input1 as u128) << 32)
        | input2 as u128;
    let ds: i128 = (input2 as i32) as i128;
    let du: u128 = ((input2 as u128) << 32) | input2 as u128;
    let m = sdiv(n as i128, ds) ^ udiv(n, du).rotate_left(17);
    (m as u32) ^ ((m >> 32) as u32)
}

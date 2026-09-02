// Signed division/remainder GUARD shapes with fully dynamic divisors that
// reach 0, -1 and MIN. i32: checked_div / checked_rem / wrapping_div /
// wrapping_rem / overflowing_div / checked_div_euclid / checked_rem_euclid
// on a fully dynamic dividend (input1) and divisor (input2). i64:
// checked_div / wrapping_div / overflowing_div (i64 `%` is compile-time
// unimplemented, see i64_srem). LLVM lowers every checked/wrapping/
// overflowing form to `rhs == 0` and `rhs == -1 && lhs == MIN` branches
// around a bare `div_s`/`rem_s`, so the VM-side `::intrinsics::i32::
// checked_div`/`wrapping_mod` and `::intrinsics::i64::checked_div` execute on
// every non-trapping pair, while the /0 and MIN/-1 pairs must take the guard
// arms (None / MIN / 0 / overflow flag). wrapping_div uses the wrapping
// negation of the dividend (MIN stays MIN) so it is not merged with the
// checked_div division; remainders use a rotated dividend so no `%` shares an
// operand pair with a `/` (LLVM would strength-reduce it to mul-sub).
// The row (0x80000000, 0xFFFFFFFF) makes i32::MIN / -1 AND i64::MIN / -1.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = input1 as i32;
    let d = input2 as i32;
    let nr = input1.rotate_left(16) as i32;
    let nn = n.wrapping_neg();

    let mut acc: u32 = 0;
    acc = acc.wrapping_add(n.checked_div(d).map_or(0x1111_1111, |q| q as u32));
    acc = acc.wrapping_add(nr.checked_rem(d).map_or(0x2222_2222, |r| r as u32).rotate_left(3));
    if d != 0 {
        acc = acc.wrapping_add((nn.wrapping_div(d) as u32).rotate_left(6));
        acc = acc.wrapping_add((nr.wrapping_rem(d) as u32).rotate_left(9));
        let (q, o) = n.overflowing_div(d);
        acc = acc.wrapping_add((q as u32).rotate_left(12)).wrapping_add(o as u32);
    }
    acc = acc.wrapping_add(n.checked_div_euclid(d).map_or(0x3333_3333, |q| q as u32).rotate_left(15));
    acc = acc.wrapping_add(nr.checked_rem_euclid(d).map_or(0x4444_4444, |r| r as u32).rotate_left(18));

    // i64: the dividend takes input1 as its high word and the INVERTED input2
    // as its low word, and the divisor is the sign-extended input2, so the row
    // (0x80000000, 0xFFFFFFFF) is exactly i64::MIN / -1 and (x, 0) is x / 0.
    let w = (((input1 as u64) << 32) | (!input2) as u64) as i64;
    let d64 = (input2 as i32) as i64;
    let mut m: u64 = w.checked_div(d64).map_or(0x5555_5555_5555_5555, |q| q as u64);
    if d64 != 0 {
        m ^= (w.wrapping_neg().wrapping_div(d64) as u64).rotate_left(7);
        let (q, o) = w.overflowing_div(d64);
        m ^= (q as u64).rotate_left(21) ^ (o as u64);
    }
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

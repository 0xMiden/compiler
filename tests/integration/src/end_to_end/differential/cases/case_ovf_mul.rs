// Overflow-detecting multiplication and related two-operand helpers at the
// MIN/MAX boundaries, all legalized by LLVM into wrapping ops + compares:
// overflowing_mul / checked_mul / saturating_mul on u64 (mul_wide_u + "hi
// word != 0"), overflowing_mul / checked_mul / saturating_mul on u32/i32
// (i64 products + range compares), plus abs_diff (compare + sub, i64 -> u64
// at MIN/MAX == 2^64-1), midpoint, and u64/i64 checked_add/checked_sub None
// arms. Every i64 overflow-checked MULTIPLY (overflowing_mul / checked_mul /
// saturating_mul) is deliberately absent: the guest LLVM miscompiles their
// `hi == lo >> 63` test in some stackifications (see sat_mul_i64 / pow_i64;
// an earlier revision of this case diverged at (0x80000000, 0) through its
// i64 overflowing_mul flag). Operands are the low/high/splat forms of each
// input, so grid rows make u64::MAX * u64::MAX, 2^63 * 2, 2^63 * 2^63,
// (2^32-1) * MAX, i64::MIN - i64::MAX and |i64::MIN - i64::MAX| == 2^64-1.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = input1 as u64;
    let a2 = a1 << 32;
    let a3 = a2 | a1;
    let b1 = input2 as u64;
    let b2 = b1 << 32;
    let b3 = b2 | b1;
    let sa2 = a2 as i64;
    let sb4 = (b3 >> 1) as i64;

    let mut m: u64 = 0;
    let (r, o) = a3.overflowing_mul(b3);
    m ^= r ^ (o as u64);
    let (r, o) = a2.overflowing_mul(b1 | 2);
    m ^= r.rotate_left(1) ^ ((o as u64) << 1);
    m ^= a3.checked_mul(b1).unwrap_or(0x1111_1111_1111_1111).rotate_left(2);
    m ^= a2.saturating_mul(b2).rotate_left(3);
    let (r, o) = a3.overflowing_mul(a3);
    m ^= r.rotate_left(4) ^ ((o as u64) << 2);
    m ^= a1.checked_mul(b3).unwrap_or(0x2222_2222_2222_2222).rotate_left(6);
    m ^= a3.saturating_mul(b1 | 1).rotate_left(7);
    m ^= sa2.abs_diff(sb4).rotate_left(10);
    m ^= a1.abs_diff(b2).rotate_left(11);
    m ^= a3.midpoint(b3).rotate_left(12);
    m ^= (sa2.midpoint(sb4) as u64).rotate_left(13);
    m ^= a3.checked_add(b1).unwrap_or(0x4444_4444_4444_4444).rotate_left(14);
    m ^= sa2.checked_sub(sb4).map_or(0x5555_5555_5555_5555, |x| x as u64).rotate_left(15);

    let mut acc: u32 = 0;
    let (r, o) = input1.overflowing_mul(input2);
    acc = acc.wrapping_add(r).wrapping_add(o as u32);
    let (r, o) = (input1 as i32).overflowing_mul(input2 as i32);
    acc = acc.wrapping_add((r as u32).rotate_left(3)).wrapping_add((o as u32) << 1);
    acc = acc.wrapping_add(input1.checked_mul(input2 | 1).unwrap_or(0x6666_6666).rotate_left(6));
    acc = acc.wrapping_add((input1 as i32).saturating_mul(input2 as i32).rotate_left(9) as u32);
    acc = acc.wrapping_add((input1 as i32).abs_diff(input2 as i32).rotate_left(12));
    acc = acc.wrapping_add(input1.midpoint(input2).rotate_left(15));
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

// Integer logarithm / root / power helpers at their value boundaries on
// u32/u64 (and a u128 ilog2): checked_ilog2 (clz-based, None at 0),
// checked_ilog10 and checked_ilog(3) (compare-chain / division loops at the
// exact powers 9/10, 99/100, 10^9, 10^19 and 3^k), isqrt (Newton loop; at
// perfect squares +-1 and at u32::MAX / u64::MAX), u32/i32/u64 checked_pow
// with a small dynamic exponent (repeated-squaring loop with overflow
// compares; i64 checked_pow is deliberately absent — LLVM miscompiles its
// loop, see pow_i64), and u64/i64/i128 abs_diff. Values are the inputs and
// their 64-bit splat forms; results fold into one accumulator.
#[inline(never)]
fn log128(w: u64, t: u64) -> u64 {
    let p: u128 = ((w as u128) << 64) | t as u128;
    let sp = p as i128;
    let l2 = p.checked_ilog2().unwrap_or(200) as u64;
    let l10 = p.checked_ilog10().unwrap_or(300) as u64;
    let d = sp.abs_diff(-(t as i128));
    l2 ^ (l10 << 8) ^ (d as u64).rotate_left(17) ^ ((d >> 64) as u64).rotate_left(29)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v = input1;
    let w: u64 = ((input1 as u64) << 32) | input2 as u64;
    let e = input2 & 7;

    let mut acc: u32 = v.checked_ilog2().unwrap_or(100);
    acc = acc.wrapping_add(v.checked_ilog10().unwrap_or(101).rotate_left(3));
    acc = acc.wrapping_add(v.checked_ilog(3).unwrap_or(102).rotate_left(6));
    acc = acc.wrapping_add(v.isqrt().rotate_left(9));
    acc = acc.wrapping_add(v.checked_pow(e).unwrap_or(0x1234_5678).rotate_left(12));
    acc = acc.wrapping_add((input1 as i32).checked_pow(e).unwrap_or(0x1234_5678 as i32) as u32);
    acc = acc.wrapping_add(v.abs_diff(input2).rotate_left(15));

    let mut m: u64 = w.checked_ilog2().unwrap_or(100) as u64;
    m ^= (w.checked_ilog10().unwrap_or(101) as u64) << 8;
    m ^= (w.checked_ilog(3).unwrap_or(102) as u64) << 16;
    m ^= w.isqrt().rotate_left(3);
    m ^= w.checked_pow(e).unwrap_or(0x9E37_79B9_7F4A_7C15).rotate_left(7);
    m ^= (w >> 3).checked_pow(e + 1).unwrap_or(0x1CED_C0FF_EE15_600D).rotate_left(11);
    m ^= (w as i64).abs_diff(input2 as i32 as i64).rotate_left(13);
    m ^= w.abs_diff(input1 as u64).rotate_left(19);
    m ^= log128(w, (input2 as u64).wrapping_mul(0x0101_0193));

    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

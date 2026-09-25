// Bit-manipulation shapes at width boundaries on u32/u64/u128 (and their
// signed twins), every value fully dynamic: swap_bytes and reverse_bits
// (LLVM expands bswap/bitreverse into shift/mask/or sequences — on u64 a
// dozen u64::shl/shr/and/or execs, on u128 per limb plus a limb swap),
// is_power_of_two (popcnt == 1), checked_next_power_of_two (`MAX >> clz(n -
// 1)` + 1 behind the n <= 1 and overflow guards), leading_ones/trailing_ones
// (clz/ctz of the complement), and wrapping_abs / unsigned_abs / checked_abs
// / signum on i32/i64/i128 at MIN (wrapping_abs(MIN) == MIN,
// unsigned_abs(MIN) == 2^(w-1), checked_abs(MIN) == None, signum -1/0/1).
// The 128-bit part lives in an `#[inline(never)]` helper; the row
// (0x80000000, 0) makes v == i32::MIN, w == i64::MIN and p == i128::MIN.
#[inline(never)]
fn fold128(p: u128) -> u64 {
    (p as u64) ^ ((p >> 64) as u64).rotate_left(29)
}

#[inline(never)]
fn bits128(w: u64, t: u64) -> u64 {
    let p: u128 = ((w as u128) << 64) | t as u128;
    let sp = p as i128;
    let mut m = fold128(p.swap_bytes());
    m ^= fold128(p.reverse_bits()).rotate_left(3);
    m ^= (p.is_power_of_two() as u64) << 1;
    m ^= ((p.leading_ones() as u64) << 2) ^ ((p.trailing_ones() as u64) << 10);
    m ^= fold128(sp.wrapping_abs() as u128).rotate_left(7);
    m ^= fold128(sp.unsigned_abs()).rotate_left(11);
    m ^= (sp.signum() as u64).rotate_left(17);
    m ^= fold128(p.checked_next_power_of_two().unwrap_or(7)).rotate_left(19);
    m ^= fold128(sp.checked_abs().map_or(0x0f0f_0f0f_0f0f_0f0f, |x| x as u128)).rotate_left(23);
    m
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v = input1;
    let w: u64 = ((input1 as u64) << 32) | input2 as u64;

    let mut acc: u32 = v.swap_bytes();
    acc = acc.wrapping_add(v.reverse_bits().rotate_left(3));
    acc = acc.wrapping_add((v.is_power_of_two() as u32) << 5);
    acc = acc.wrapping_add(v.checked_next_power_of_two().unwrap_or(0x1234_5678).rotate_left(7));
    acc = acc.wrapping_add((v.leading_ones() << 8) ^ (v.trailing_ones() << 16));
    let s = v as i32;
    acc = acc.wrapping_add((s.wrapping_abs() as u32).rotate_left(11));
    acc = acc.wrapping_add(s.unsigned_abs().rotate_left(13));
    acc = acc.wrapping_add((s.signum() as u32).rotate_left(17));
    acc = acc.wrapping_add(s.checked_abs().map_or(0x0f0f_0f0f, |x| x as u32).rotate_left(19));

    let mut m: u64 = w.swap_bytes();
    m ^= w.reverse_bits().rotate_left(3);
    m ^= (w.is_power_of_two() as u64) << 1;
    m ^= w.checked_next_power_of_two().unwrap_or(0x9E37_79B9_7F4A_7C15).rotate_left(5);
    m ^= ((w.leading_ones() as u64) << 8) ^ ((w.trailing_ones() as u64) << 16);
    let sw = w as i64;
    m ^= (sw.wrapping_abs() as u64).rotate_left(11);
    m ^= sw.unsigned_abs().rotate_left(13);
    m ^= (sw.signum() as u64).rotate_left(17);
    m ^= sw.checked_abs().map_or(0x0f0f_0f0f_0f0f_0f0f, |x| x as u64).rotate_left(19);

    m ^= bits128(w, (input1 as u64).wrapping_mul(input2 as u64));
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

// 128-bit shift shapes at count boundaries, count = input2 unmasked:
// u128/i128 checked_shl / checked_shr / overflowing_shl / overflowing_shr
// (LLVM: `count < 128` compare + select around the __ashlti3/__lshrti3/
// __ashrti3 libcalls, so counts 128/129/u32::MAX take the None / flag arms),
// i128 wrapping_shl of a NEGATIVE value by counts >= 64 (the low limb moves
// into the high limb, the low limb becomes zero) and wrapping_shr (logical
// on the u128 view vs arithmetic on the i128 view of the same bits), u8
// checked_shl (None from count 8) and u16 checked_shr (None from 16), and
// u64 shifts whose count comes from a u64 (`wrapping_shl(c64 as u32)`).
// Lives in an `#[inline(never)]` helper to keep the live pressure low.
#[inline(never)]
fn fold128(p: u128) -> u64 {
    (p as u64) ^ ((p >> 64) as u64).rotate_left(29)
}

#[inline(never)]
fn sh128(w: u64, c: u32) -> u64 {
    let p: u128 = ((w as u128) << 64) | (w ^ 0x9E37_79B9_7F4A_7C15) as u128;
    let sp = p as i128;
    let mut m = fold128(p.checked_shl(c).unwrap_or(0x1111_1111_1111_1111_1111_1111_1111_1111));
    m ^= fold128(p.checked_shr(c).unwrap_or(0x2222_2222_2222_2222_2222_2222_2222_2222)).rotate_left(1);
    m ^= fold128(sp.checked_shr(c).map_or(0x3333_3333_3333_3333_3333_3333_3333_3333, |x| x as u128))
        .rotate_left(2);
    let (r, o) = p.overflowing_shl(c);
    m ^= fold128(r).rotate_left(3) ^ (o as u64);
    let (r, o) = sp.overflowing_shr(c);
    m ^= fold128(r as u128).rotate_left(4) ^ ((o as u64) << 1);
    m ^= fold128(sp.wrapping_shl(c) as u128).rotate_left(5);
    m ^= fold128(sp.wrapping_shr(c) as u128).rotate_left(6);
    m ^= fold128(p.wrapping_shr(c)).rotate_left(7);
    m
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let c = input2;
    let w: u64 = ((input1 as u64) << 32) | (input1 ^ 0x5bd1_e995) as u64;
    let c64: u64 = (input2 as u64) | ((input1 as u64 & 1) << 32);

    let mut acc: u32 = (input1 as u8).checked_shl(c).map_or(0x0101_0101, |x| x as u32);
    acc = acc.wrapping_add((input1 as u16).checked_shr(c).map_or(0x0202_0202, |x| x as u32).rotate_left(5));
    acc = acc.wrapping_add((input1 as i8).checked_shr(c).map_or(0x0303_0303, |x| x as i32 as u32).rotate_left(9));
    let mut m: u64 = w.wrapping_shl(c64 as u32);
    m ^= w.wrapping_shr((c64 >> 1) as u32).rotate_left(3);
    m ^= (w as i64).wrapping_shr(c64 as u32).rotate_left(7) as u64;
    m ^= sh128(w, c);
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

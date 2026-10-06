// Wide multiplication and carry propagation at operand boundaries. Each
// input is used in three 64-bit forms — low word `x`, high word `x << 32`
// and splat `x << 32 | x` — so single grid rows make u64::MAX * u64::MAX,
// 2^63 * 2^63, (2^32-1) * 2^63, i64::MIN * i64::MIN, i64::MIN * -1, -1 * -1
// and i64::MIN * i64::MAX through `i64.mul_wide_u` / `i64.mul_wide_s`
// (zext/sext to i128, `u128::wrapping_mul`, limb split), with the hi and lo
// words of every product folded. The `#[inline(never)]` helper then runs
// full 4-limb u128 x u128 products in the core-lib wrapping_mul (limb-swapped
// operands: MAX * MAX == 1; a parity-selected 2^64 gives (2^64-1) * (2^64+1)
// == MAX), a multiply-add chain whose carries ripple through all four limbs
// (`u128::wrapping_add`/`wrapping_sub`: MAX + 1 == 0, 0 - 1 == MAX), and an
// i128 product of a negated operand.
#[inline(never)]
fn fold128(p: u128) -> u64 {
    (p as u64) ^ ((p >> 64) as u64).rotate_left(29)
}

#[inline(never)]
fn wide_products(a2: u64, a3: u64, b2: u64, b3: u64, t: u64) -> u64 {
    let p: u128 = ((a3 as u128) << 64) | b3 as u128;
    let q: u128 = ((b3 as u128) << 64) | a3 as u128;
    let big: u128 = ((t & 1) as u128) << 64; // 2^64 on odd t, else 0
    let mut m = fold128(p.wrapping_mul(q));
    m ^= fold128(big.wrapping_sub(1).wrapping_mul(big | 1)).rotate_left(3);
    m ^= fold128(p.wrapping_add(q).wrapping_mul(big.wrapping_add(a2 as u128)).wrapping_add(q))
        .rotate_left(7);
    m ^= fold128(p.wrapping_add(1).wrapping_sub(q)).rotate_left(11);
    m ^= fold128((p as i128).wrapping_neg().wrapping_mul(q as i128) as u128).rotate_left(13);
    m
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = input1 as u64;
    let a2 = a1 << 32;
    let a3 = a2 | a1;
    let b1 = input2 as u64;
    let b2 = b1 << 32;
    let b3 = b2 | b1;

    // Unsigned widening products (mul_wide_u).
    let mut m: u64 = fold128((a3 as u128) * (b3 as u128)); // MAX * MAX
    m ^= fold128((a2 as u128) * (b2 as u128)).rotate_left(1); // 2^63 * 2^63
    m ^= fold128((a1 as u128) * (b2 as u128)).rotate_left(2); // (2^32-1) * 2^63
    m ^= fold128((a3 as u128) * (b1 as u128)).rotate_left(3); // MAX * (2^32-1)
    m ^= a3.wrapping_mul(b3).rotate_left(4); // plain u64 wrapping product

    // Signed widening products (mul_wide_s).
    let sa2 = a2 as i64; // i64::MIN at input1 == 0x80000000
    let sa3 = a3 as i64; // -1 at input1 == u32::MAX
    let sb2 = b2 as i64;
    let sb3 = b3 as i64;
    let sb4 = (b3 >> 1) as i64; // i64::MAX at input2 == u32::MAX
    m ^= fold128(((sa2 as i128) * (sb2 as i128)) as u128).rotate_left(5); // MIN * MIN
    m ^= fold128(((sa2 as i128) * (sb3 as i128)) as u128).rotate_left(6); // MIN * -1
    m ^= fold128(((sa3 as i128) * (sb3 as i128)) as u128).rotate_left(7); // -1 * -1
    m ^= fold128(((sa2 as i128) * (sb4 as i128)) as u128).rotate_left(8); // MIN * MAX
    m ^= fold128(((sa3 as i128) * (sb4 as i128)) as u128).rotate_left(9); // -1 * MAX

    m ^= wide_products(a2, a3, b2, b3, a1 ^ b1);
    (m as u32) ^ ((m >> 32) as u32)
}

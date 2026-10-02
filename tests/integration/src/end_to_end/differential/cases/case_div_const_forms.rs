// Constant-divisor forms next to generic division. On wasm LLVM does NOT
// strength-reduce constant divisors (isIntDivCheap): `x / C` and `x % C`
// stay `div_s/div_u/rem_s/rem_u` with an immediate operand, and only the
// UNSIGNED power-of-two divisors (2^32, 2^63 here) become `shr_u`/`and`
// (wat-verified; no multiply-high magic forms exist on this target). So the
// immediate-operand divisions below are folded together with the SAME
// quotient computed against an opaquely equal runtime divisor (`k + z`, `z`
// an unfoldable cross-modulus zero), both going through the checked_div/
// checked_mod emitters (u32div/u32mod, u64::div/u64::mod, i32/i64
// checked_div, i32 wrapping_mod) — a mismatch on either side diverges.
// Signed dividends are fully dynamic (MIN, -1, 0 included); nothing traps.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Opaque zero: `input2 % 6 == 5` and `input2 % 3 == 0` cannot both hold.
    let z = ((input2 % 6 == 5) & (input2 % 3 == 0)) as u32;

    let n = input1 as i32;
    let w = (((input1 as u64) << 32) | input2 as u64) as i64;
    let u = input1;
    let x = ((input2 as u64) << 32) | input1 as u64;

    let mut acc: u32 = 0;

    // Signed i32 by powers of two (shift + bias) vs runtime divisor.
    acc = acc.wrapping_add((n / 8) as u32);
    acc = acc.wrapping_add((n / (8 + z as i32)) as u32);
    acc = acc.wrapping_add((n % 16) as u32);
    acc = acc.wrapping_add((n % (16 + z as i32)) as u32);
    acc = acc.wrapping_add((n / 7) as u32);
    acc = acc.wrapping_add((n / (7 + z as i32)) as u32);
    acc = acc.wrapping_add((n % 10) as u32);
    acc = acc.wrapping_add((n % (10 + z as i32)) as u32);

    // Unsigned u32 by constants vs runtime divisor.
    acc = acc.wrapping_add(u / 10);
    acc = acc.wrapping_add(u / (10 + z));
    acc = acc.wrapping_add(u % 1000);
    acc = acc.wrapping_add(u % (1000 + z));

    // Signed i64: powers of two, small magic divisors, a wide divisor.
    let mut m: u64 = 0;
    m ^= (w / 32) as u64;
    m ^= ((w / (32 + z as i64)) as u64).rotate_left(1);
    m ^= ((w / 7) as u64).rotate_left(2);
    m ^= ((w / (7 + z as i64)) as u64).rotate_left(3);
    m ^= ((w / 1000) as u64).rotate_left(4);
    m ^= ((w / (1000 + z as i64)) as u64).rotate_left(5);
    m ^= ((w / 641) as u64).rotate_left(6);
    m ^= ((w / (641 + z as i64)) as u64).rotate_left(7);
    m ^= ((w / 0x1_0000_0001) as u64).rotate_left(8);
    m ^= ((w / (0x1_0000_0001 + z as i64)) as u64).rotate_left(9);

    // Unsigned u64: powers of two (including the limb boundary), magic
    // divisors whose 64-bit magic has the top bit set (10, 3), a remainder.
    m ^= (x / 0x1_0000_0000).rotate_left(10);
    m ^= (x / (0x1_0000_0000 + z as u64)).rotate_left(11);
    m ^= (x / 10).rotate_left(12);
    m ^= (x / (10 + z as u64)).rotate_left(13);
    m ^= (x / 3).rotate_left(14);
    m ^= (x / (3 + z as u64)).rotate_left(15);
    m ^= (x % 1000).rotate_left(16);
    m ^= (x % (1000 + z as u64)).rotate_left(17);
    m ^= (x % 0x8000_0000_0000_0000).rotate_left(18);
    m ^= (x % (0x8000_0000_0000_0000 + z as u64)).rotate_left(19);

    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

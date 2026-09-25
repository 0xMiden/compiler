// C12 value ladders x multi-exit loops (campaign 14): inside a five-exit
// loop nest (outer `while i < input2 % 31` zero-trip-capable, inner
// bottom-test), every exit is decided by a value-ladder result computed on
// the trip: an i32 `checked_div` with a dynamic divisor that reaches 0, -1
// and MIN (None -> labeled break with a value), a `checked_shl` at runtime
// counts crossing the width (None -> early return), the high limb of a
// `mul_wide_u` product (zero -> inner break), an i16 sext chain deciding the
// post-inner break, and a 4-limb u128 product folded into the loop-carried
// u64 on every trip. No i64 checked/saturating multiply (guest-toolchain
// class). Exit tag = top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut x = input1 | 1;
    let mut w: u64 = ((input1 as u64) << 32) | input2 as u64;
    let n = input2 % 31;
    let jn = (input1 % 4).wrapping_add(2);
    let mut i = 0u32;
    let mut j = 0u32;
    let r = 'outer: loop {
        if i >= n {
            break (1 << 28) | (x & 0x0fff_ffff);
        }
        j = 0;
        while j < jn {
            // The divisor cycles through input2, -input2, input2 ^ MIN and -1.
            let d = match j & 3 {
                0 => input2 as i32,
                1 => (input2 as i32).wrapping_neg(),
                2 => (input2 ^ 0x8000_0000) as i32,
                _ => -1i32,
            };
            let nn = (x as i32) ^ (i as i32);
            let q = match nn.checked_div(d) {
                Some(q) => q,
                None => break 'outer (2 << 28) | ((x ^ (d as u32)) & 0x0fff_ffff),
            };
            // Runtime shift count 0..39: None for 32..39.
            let k = ((input1 >> (8 * (j & 3))) & 0xff) % 40;
            let sh = match x.checked_shl(k) {
                Some(v) => v,
                None => return (3 << 28) | ((x.wrapping_add(k) ^ (q as u32)) & 0x0fff_ffff),
            };
            // mul_wide_u: high limb of a 64x64 product.
            let p: u128 = (w as u128) * (((x as u64) | 1) as u128);
            let hi = (p >> 64) as u32;
            if hi == 0 {
                x = x.rotate_left(5) ^ (p as u32);
                break;
            }
            // sext/trunc chain from the low byte and halfword.
            let s = (((x as u8) as i8 as i64) as u64).rotate_left(k & 63)
                ^ ((((sh as i16) as i32) as u32) as u64);
            // 4-limb u128 product with the lanes assembled from the ladder.
            let four: u128 =
                (((w as u128) << 64) | (s as u128)).wrapping_mul(((p as u64) as u128) | 1);
            w = (four as u64) ^ ((four >> 64) as u64).rotate_left(11) ^ w.rotate_left(1);
            x = x.wrapping_mul(0x0100_0193) ^ (q as u32) ^ sh ^ (s as u32) ^ hi;
            j = j.wrapping_add(1);
        }
        if (((x as i16) as i32 as u32) ^ ((w >> 32) as u32)) & 0x7_0000 == 0x3_0000 {
            break (4 << 28) | ((j.wrapping_mul(0x0101_0101) ^ (w as u32)) & 0x0fff_ffff);
        }
        i = i.wrapping_add(1);
    };
    (r & 0xf000_0000)
        | ((r ^ (w as u32) ^ ((w >> 32) as u32) ^ i.wrapping_mul(0x9e37_79b9)) & 0x0fff_ffff)
}

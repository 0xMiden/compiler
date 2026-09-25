// Sign-extension / truncation chains through `as` casts in orders the corpus
// never formed, every source value fully dynamic and every result folded
// into an accumulator right away (low live pressure):
// - i8 -> u64 (`sext_smallint(8, 64)` then unsigned reinterpretation) and
//   u8 -> i64 (zext);
// - i16 -> i64 followed by an arithmetic `>> 40` (pure sign fill);
// - extension of a value whose high bits were set by arithmetic (a wrapping
//   product truncated to i16, then extended to i64);
// - `i64::from(i32) * i64::from(i32)` (two `sext_int32(64)` feeding the u64
//   wrapping multiply) at MIN * MIN / MIN * -1 / MAX * MAX;
// - sext followed by a LOGICAL shift (`as i64 as u64 >> 33`);
// - i8 * i8 products in i32, the high limb of a u64 taken through i8 -> i64,
//   an i32 -> i64 -> i32 round trip after a 64-bit add, i64 x i16 products
//   in i128 (sign-extended i64 pairs into `i64.mul_wide_s`), and i16/i8/u8
//   slices of one u64 extended to i64.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = input1;
    let y = input2;
    let w: u64 = ((input1 as u64) << 32) | input2 as u64;

    let mut m: u64 = (x as i8) as u64; // sext 8 -> 64, reinterpret
    m ^= ((y as u8) as i64 as u64).rotate_left(3); // zext 8 -> 64
    m ^= ((((x as i16) as i64) >> 40) as u64).rotate_left(6); // sext 16 -> 64, sign fill
    m ^= (((x.wrapping_mul(0x9E37_79B9) as i16) as i64).wrapping_add(1) as u64).rotate_left(9);
    m ^= (i64::from(x as i32).wrapping_mul(i64::from(y as i32)) as u64).rotate_left(12);
    m ^= ((((y as i32) as i64) as u64) >> 33).rotate_left(15); // sext then logical shift
    m ^= ((((w >> 32) as i8) as i64) as u64).rotate_left(18); // high limb -> i8 -> i64
    m ^= ((((w as i16) as i64) ^ ((w as i8) as i64) ^ (((w >> 16) as u8) as i64)) as u64)
        .rotate_left(21);
    let j: i128 = (x as i32 as i64 as i128).wrapping_mul((y as i16) as i128);
    m ^= (j as u64).rotate_left(24) ^ ((j >> 64) as u64).rotate_left(27);

    let g: i32 = (x as i8 as i32).wrapping_mul(y as i8 as i32); // i8 * i8 in i32
    let i: i32 = ((y as i32) as i64).wrapping_add(w as i64) as i32; // i64 round trip
    (g as u32)
        .wrapping_add((i as u32).rotate_left(5))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

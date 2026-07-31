// Exercises zero/sign-extending sub-word loads directly into 64-bit values,
// with runtime indexes so nothing folds: `i64.load8_u`/`i64.load16_u`/
// `i64.load32_u` translate to a U8/U16/U32-typed `hir.load` + `arith.zext` to
// U64, reaching the 64-bit arms of `zext_smallint` and `zext_int32`, while
// `i64.load8_s`/`i64.load16_s`/`i64.load32_s` lower via `load` + `sext`
// (memory-flavored `sext_smallint`/`sext_int32` entries).
static BYTES: [u8; 16] = [
    0x81, 0x11, 0xF3, 0x07, 0x5A, 0xC2, 0x39, 0xEE, 0x40, 0x9D, 0x02, 0x77, 0xB8, 0x1F, 0x64, 0x8C,
];
static WORDS: [u16; 8] = [0x8001, 0x1234, 0xFFFE, 0x0042, 0xA55A, 0x0F0F, 0xC3C3, 0x7FFF];
static DWORDS: [u32; 4] = [0x8000_0001, 0x1234_5678, 0xDEAD_BEEF, 0x0BAD_F00D];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = (input1 & 15) as usize;
    let j = (input2 & 7) as usize;
    let k = ((input1 >> 4) & 3) as usize;

    let zu8: u64 = BYTES[i] as u64; // i64.load8_u
    let zu16: u64 = WORDS[j] as u64; // i64.load16_u
    let zu32: u64 = DWORDS[k] as u64; // i64.load32_u
    let s8: i64 = (BYTES[i ^ 1] as i8) as i64; // i64.load8_s
    let s16: i64 = (WORDS[j ^ 1] as i16) as i64; // i64.load16_s
    let s32: i64 = (DWORDS[k ^ 1] as i32) as i64; // i64.load32_s

    let m = zu8
        .wrapping_add(zu16)
        .wrapping_add(zu32)
        .wrapping_add(s8 as u64)
        ^ (s16 as u64)
        ^ (s32 as u64);
    (m as u32) ^ ((m >> 32) as u32)
}

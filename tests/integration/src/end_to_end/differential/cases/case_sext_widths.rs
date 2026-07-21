// Exercises the sign-extension width-conversion chain: wasm
// `i32.extend8_s`/`i32.extend16_s`/`i64.extend8_s`/`i64.extend16_s`/
// `i64.extend32_s` translate to `wasm.SignExtend`, which the MASM backend
// lowers as `trunc(src_ty)` + `sext(dst_ty)` — covering the small-width arms
// of `trunc_int32`/`trunc_int64` plus `sext_smallint` (8/16 -> 32/64), and
// `i64.extend_i32_s` reaches `sext_int32(64)` directly. No i128 shapes, so
// this stays clear of the known sext_shapes/mul_wide_s divergence.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x: u64 = ((input1 as u64) << 32) | input2 as u64;

    let s8_32: i32 = (input1 as i8) as i32; // i32.extend8_s
    let s16_32: i32 = (input2 as i16) as i32; // i32.extend16_s
    let s8_64: i64 = (input2 as i8) as i64; // i64.extend8_s
    let s16_64: i64 = (input1 as i16) as i64; // i64.extend16_s
    let s32_64: i64 = (input1 as i32) as i64; // i64.extend_i32_s
    let sx: i64 = (x as i32) as i64; // i64.extend32_s (source already 64-bit)

    let t = (s8_64 ^ s16_64 ^ s32_64 ^ sx) as u64;
    (s8_32 as u32)
        .wrapping_add(s16_32 as u32)
        .wrapping_add(t as u32)
        .wrapping_add((t >> 32) as u32)
}

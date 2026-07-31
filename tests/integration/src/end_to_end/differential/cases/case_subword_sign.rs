// Sub-word sign boundaries: i8 0x7F/0x80/0xFF and i16 0x7FFF/0x8000/0xFFFF,
// both via sign-extending table loads (i32/i64.load8_s/load16_s at
// grid-pinned indexes hitting the exact MIN/MAX/-1 entries) and via
// `as i8 as i32` / `as i16 as i64` truncate-then-sign-extend chains
// (extend8_s/extend16_s) on grid-pinned boundary bytes — including rows that
// assert truncation happens BEFORE sign extension (0x100 -> 0, 0xFF80 -> -128).
static SB: [i8; 8] = [127, -128, -1, 0, 1, -127, 100, -100];

static SH: [i16; 8] = [32767, -32768, -1, 0, 1, -32767, 1000, -1000];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let bi = (input1 % 8) as usize;
    let hi = (input2 % 8) as usize;

    let a = SB[bi] as i32; // i32.load8_s
    let b = SB[hi] as i64; // i64.load8_s
    let c = SH[hi] as i32; // i32.load16_s
    let d = SH[bi] as i64; // i64.load16_s

    let e = input1 as i8 as i32; // extend8_s at boundary bytes
    let f = input2 as i16 as i32; // extend16_s at boundary halfwords
    let g = input1 as i8 as i64; // trunc + sext straight to 64
    let h = input2 as i16 as i64;

    let m = (b as u64)
        ^ (d as u64).rotate_left(7)
        ^ (g as u64).rotate_left(13)
        ^ (h as u64).rotate_left(19);
    (a as u32)
        .wrapping_add((c as u32).rotate_left(5))
        .wrapping_add((e as u32).rotate_left(9))
        .wrapping_add((f as u32).rotate_left(17))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

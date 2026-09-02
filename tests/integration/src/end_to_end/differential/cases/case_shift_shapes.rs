// Shift/rotate shapes at count boundaries the wrapping_* corpus cases never
// form, with count = input2 (unmasked) and value = input1 (plus derived
// 64/128-bit values), so one grid row pins each count boundary:
// - checked_shl/checked_shr/overflowing_shl/overflowing_shr on u32/i32/u64/
//   i64: LLVM emits a `count < width` compare + select around the masked
//   shift, so counts >= width must take the None / overflow-flag arms while
//   the shift itself still executes with the masked count;
// - sub-word wrapping_shl/wrapping_shr/rotate on u8/u16/i8/i16: Rust masks
//   the count % 8 / % 16, LLVM shifts as i32 and re-masks (or sign-extends);
// - u128 rotate_left/rotate_right by a runtime count (masked % 128): LLVM's
//   funnel-shift expansion over the __ashlti3/__lshrti3 libcalls, crossing
//   the 64-bit limb boundary at 63/64/65 and the identity at 0/128.
// The 128-bit rotates live in an `#[inline(never)]` helper to keep the
// entrypoint's live pressure low.
#[inline(never)]
fn rot128(w: u64, c: u32) -> u64 {
    let p: u128 = ((w as u128) << 64) | (w.rotate_left(13) ^ (c as u64)) as u128;
    let r1 = p.rotate_left(c);
    let r2 = p.rotate_right(c);
    let q = r1 ^ r2.rotate_left(7);
    (q as u64) ^ ((q >> 64) as u64).rotate_left(23)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let c = input2;
    let v = input1;
    let s = input1 as i32;
    let w: u64 = ((input1 as u64) << 32) | (input1 ^ 0x5bd1_e995) as u64;
    let sw = w as i64;

    let mut acc: u32 = 0;
    acc = acc.wrapping_add(v.checked_shl(c).unwrap_or(0x1111_1111));
    acc = acc.wrapping_add(v.checked_shr(c).unwrap_or(0x2222_2222).rotate_left(3));
    acc = acc.wrapping_add(s.checked_shr(c).map_or(0x3333_3333, |x| x as u32).rotate_left(6));
    let (r, o) = v.overflowing_shl(c);
    acc = acc.wrapping_add(r.rotate_left(9)).wrapping_add(o as u32);
    let (r, o) = s.overflowing_shr(c);
    acc = acc.wrapping_add((r as u32).rotate_left(12)).wrapping_add((o as u32) << 1);

    // Sub-word shifts and rotates (count masked % 8 / % 16 by Rust).
    let b = input1 as u8;
    let h = input1 as u16;
    acc = acc.wrapping_add((b.wrapping_shl(c) as u32).rotate_left(15));
    acc = acc.wrapping_add((h.wrapping_shr(c) as u32).rotate_left(18));
    acc = acc.wrapping_add((b.rotate_left(c) as u32).rotate_left(21));
    acc = acc.wrapping_add((h.rotate_right(c) as u32).rotate_left(24));
    acc = acc.wrapping_add(((input1 as i8).wrapping_shr(c) as i32 as u32).rotate_left(27));
    acc = acc.wrapping_add(((input1 as i16).wrapping_shl(c) as i32 as u32).rotate_left(30));

    // 64-bit checked/overflowing forms.
    let mut m: u64 = w.checked_shl(c).unwrap_or(0x4444_4444_4444_4444);
    m ^= sw.checked_shr(c).map_or(0x5555_5555_5555_5555, |x| x as u64).rotate_left(5);
    let (r, o) = w.overflowing_shr(c);
    m ^= r.rotate_left(11) ^ (o as u64);
    let (r, o) = sw.overflowing_shl(c);
    m ^= (r as u64).rotate_left(17) ^ ((o as u64) << 1);

    m ^= rot128(w, c);
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

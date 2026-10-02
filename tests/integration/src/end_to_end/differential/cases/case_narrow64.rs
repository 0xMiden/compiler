// Narrow stores of 64-bit values and widening loads back, at runtime byte
// offsets 0..3: `i64.store8`/`i64.store16`/`i64.store32` (a u64 truncated on
// the way into u8/u16/u32 slots of a byte buffer — `trunc_int64` feeding
// `store_small`/`store_u16`/`store_sw` at unaligned addresses) and the
// widening loads `i64.load8_u/8_s/16_u/16_s/32_u/32_s` at other runtime
// offsets (the reads use a second offset LLVM cannot prove equal to the
// write offset, so no store is forwarded to a load). The buffer is hashed
// whole at the end so a narrow store that clobbers a neighbour shows.
#[repr(C, align(8))]
struct Buf([u8; 48]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0; 48]);
    let mut k = 0usize;
    while k < 48 {
        b.0[k] = (input2 >> (k & 7)) as u8 ^ (k as u8).wrapping_mul(11);
        k += 1;
    }
    let q = ((input1 as u64) << 32 | input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let r = q.rotate_left(input2 & 63) ^ 0xff00_ff00_00ff_00ff;
    let o = (input1 & 3) as usize;
    let p = ((input2 >> 5) & 3) as usize;

    // Narrow stores from 64-bit values at offsets o..o+3 within each slot.
    b.0[o] = q as u8;
    b.0[o + 5] = (q >> 8) as u8;
    b.0[o + 8..o + 10].copy_from_slice(&((q >> 16) as u16).to_le_bytes());
    b.0[o + 13..o + 17].copy_from_slice(&((q >> 24) as u32).to_le_bytes());
    b.0[o + 20..o + 22].copy_from_slice(&(r as u16).to_le_bytes());
    b.0[o + 24..o + 28].copy_from_slice(&(r as u32).to_le_bytes());
    b.0[o + 30] = (r >> 56) as u8;
    b.0[o + 33..o + 35].copy_from_slice(&((r >> 40) as u16).to_le_bytes());
    b.0[o + 36..o + 40].copy_from_slice(&((r >> 20) as u32).to_le_bytes());

    // Widening loads at the second offset.
    let z8 = b.0[p + 5] as u64;
    let s8 = (b.0[p] as i8) as i64;
    let z16 = u16::from_le_bytes(b.0[p + 8..p + 10].try_into().unwrap()) as u64;
    let s16 = i16::from_le_bytes(b.0[p + 20..p + 22].try_into().unwrap()) as i64;
    let z32 = u32::from_le_bytes(b.0[p + 13..p + 17].try_into().unwrap()) as u64;
    let s32 = i32::from_le_bytes(b.0[p + 24..p + 28].try_into().unwrap()) as i64;
    let s16b = i16::from_le_bytes(b.0[p + 33..p + 35].try_into().unwrap()) as i64;
    let z32b = u32::from_le_bytes(b.0[p + 36..p + 40].try_into().unwrap()) as u64;

    let mut m = z8.wrapping_mul(0x0101_0101_0101_0101) ^ (s8 as u64).rotate_left(7);
    m = m.wrapping_add(z16 << 3) ^ (s16 as u64).rotate_left(13);
    m = m.wrapping_add(z32.rotate_left(29)) ^ (s32 as u64).rotate_left(41);
    m = m.wrapping_add((s16b as u64) << 5) ^ z32b.rotate_left(17);
    let mut acc = (m as u32) ^ ((m >> 32) as u32);
    k = 0;
    while k < 48 {
        acc = acc.rotate_left(1) ^ (b.0[k] as u32).wrapping_mul(k as u32 | 1);
        k += 1;
    }
    acc
}

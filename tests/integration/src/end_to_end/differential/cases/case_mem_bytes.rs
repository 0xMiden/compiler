// Sub-word and unaligned memory access. Signed table reads produce
// i32.load8_s/load16_s and i64.load8_s/load16_s (sext_smallint, both the
// 32-bit and >32-bit extension paths); `from_le_bytes`/`to_le_bytes` on
// byte slices at runtime-odd offsets become single unaligned
// i32/i64.load/store, exercising the cross-element load/store arms in
// `codegen/masm/src/emit/mem.rs`.
static SBYTES: [i8; 16] = [-128, -7, 13, -1, 0, 127, -64, 5, -100, 99, -2, 33, -77, 8, -31, 64];

static SHORTS: [i16; 8] = [-32768, -1, 257, 32767, -12345, 513, -300, 1000];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Sign-extending sub-word loads, widened to both i32 and i64.
    let a = SBYTES[(input1 % 16) as usize] as i32;
    let b = SHORTS[(input2 % 8) as usize] as i32;
    let c = SBYTES[(input2 % 16) as usize] as i64;
    let d = SHORTS[(input1 % 8) as usize] as i64;

    let mut buf = [0u8; 40];
    let mut i = 0u32;
    while i < 40 {
        buf[i as usize] = (input1.wrapping_mul(i).wrapping_add(input2 >> (i % 8))) as u8;
        i += 1;
    }

    // Unaligned loads at offset 1..=4.
    let off = ((input2 % 4) + 1) as usize;
    let w = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
    let h = u16::from_le_bytes(buf[off + 5..off + 7].try_into().unwrap()) as u32;
    let q = u64::from_le_bytes(buf[off + 8..off + 16].try_into().unwrap());

    // Unaligned stores at the same odd offsets.
    let x = w ^ input1.rotate_left(7);
    buf[off + 17..off + 21].copy_from_slice(&x.to_le_bytes());
    buf[off + 22..off + 24].copy_from_slice(&(h as u16 ^ 0x5a5a).to_le_bytes());
    buf[off + 24..off + 32].copy_from_slice(&q.wrapping_mul(0x9e3779b97f4a7c15).to_le_bytes());
    let r = buf[(input1 % 40) as usize] as u32;

    let s = (a.wrapping_add(b) as i64).wrapping_add(c.wrapping_mul(d)) as u32;
    s ^ w ^ (h << 8) ^ (q as u32) ^ r
}

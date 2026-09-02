// Word-straddling wide values in an `align(16)` byte buffer: u64 at byte
// offsets 4 mod 8 (LLVM proves 4-byte alignment -> `i64.load/store`
// align=2, byte-space `load_dw`/`store_dw` at an odd element address with
// byte offset 0), u128 at offsets 4/8/12 (two i64 halves each, straddling
// Miden words), i64 at odd byte offsets (three-element reassembly, then a
// sign-dependent arithmetic shift), and u64 at every byte offset 0..7. Each
// wide value is written back at a different straddling offset, and a final
// walk over all 64 bytes exposes any clobbered neighbour.
#[repr(C, align(16))]
struct Buf([u8; 64]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0; 64]);
    let mut k = 0usize;
    while k < 64 {
        b.0[k] = (input1.wrapping_mul(k as u32 + 1) ^ (input2 >> (k & 15))) as u8;
        k += 1;
    }

    // u64 at 4 mod 8: offsets 4, 12, 20, 28.
    let o8 = 4 + 8 * (input1 & 3) as usize;
    let q = u64::from_le_bytes(b.0[o8..o8 + 8].try_into().unwrap());
    // u128 at 4/8/12/16.
    let o16 = 4 * (1 + (input2 & 3)) as usize;
    let w = u128::from_le_bytes(b.0[o16..o16 + 16].try_into().unwrap());
    // i64 at an odd byte offset 1/3/5/7 (arithmetic shift makes the sign
    // of the reassembled value observable).
    let oo = (1 + 2 * ((input1 >> 2) & 3)) as usize;
    let s = i64::from_le_bytes(b.0[oo..oo + 8].try_into().unwrap()) >> 5;
    // u64 at any byte offset 0..7 within the second half.
    let oa = 32 + ((input2 >> 2) as usize & 7);
    let u = u64::from_le_bytes(b.0[oa..oa + 8].try_into().unwrap());

    let mut acc = (q as u32) ^ ((q >> 32) as u32).rotate_left(7);
    acc ^= (w as u32) ^ ((w >> 32) as u32) ^ ((w >> 64) as u32).rotate_left(3) ^ ((w >> 96) as u32);
    acc = acc.wrapping_add(s as u32).wrapping_add((s >> 32) as u32);
    acc = acc.wrapping_add(u as u32 ^ (u >> 32) as u32);

    // Write the wide values back at other straddling offsets.
    let q2 = q.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (acc as u64);
    let so8 = 28 + 8 * ((input2 >> 4) & 3) as usize; // 28/36/44/52 -> 4 mod 8
    b.0[so8..so8 + 8].copy_from_slice(&q2.to_le_bytes());
    let w2 = w.wrapping_mul(0x0000_0001_0000_0003_0000_0005_0000_0007) ^ (q2 as u128);
    let so16 = 20 + 4 * ((input1 >> 6) & 3) as usize; // 20/24/28/32
    b.0[so16..so16 + 16].copy_from_slice(&w2.to_le_bytes());
    let s2 = (s ^ (acc as i64)).wrapping_mul(-7);
    let soo = 41 + 2 * ((input2 >> 6) & 3) as usize; // 41/43/45/47
    b.0[soo..soo + 8].copy_from_slice(&s2.to_le_bytes());
    let sou = 8 + ((input1 >> 9) as usize & 7); // 8..15
    b.0[sou..sou + 8].copy_from_slice(&u.rotate_left(13).to_le_bytes());

    // Read one straddling u128 back across the rewritten region.
    let rb = u128::from_le_bytes(b.0[so16..so16 + 16].try_into().unwrap());
    acc = acc.wrapping_add((rb >> 64) as u32).wrapping_add((rb >> 8) as u32);

    let mut i = 0usize;
    while i < 64 {
        acc = acc.rotate_left(1) ^ (b.0[i] as u32).wrapping_mul(i as u32 | 1);
        i += 1;
    }
    acc
}

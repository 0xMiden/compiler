// Sub-word lanes inside 32-bit words. Every byte lane of four words is
// written through `write_volatile` (volatile stores cannot be merged into
// word stores by LLVM, so each one is an `i32.store8` at lane 0/1/2/3), the
// words are then read back WHOLE as u32 (element-space loads of a byte
// buffer) and the lanes are read back in a permuted order (byte loads at
// runtime lanes). Halfword stores land at byte offsets 0/1/2/3 (odd =
// unaligned, 3 = element-straddling), bytes are read out of a u64 stored to
// the frame at a runtime lane, and negative i8/i16 lanes are sign-extended
// through both 32- and 64-bit loads. The final hash walks every byte so a
// clobbered neighbour lane changes the result.
use core::ptr;

#[repr(C, align(8))]
struct Lanes {
    bytes: [u8; 16],
    halves: [u8; 16],
    signed: [i8; 8],
    shorts: [i16; 8],
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut l = Lanes {
        bytes: [0; 16],
        halves: [0; 16],
        signed: [0; 8],
        shorts: [0; 8],
    };

    // Byte stores at every lane of four words, values derived per lane.
    let mut k = 0usize;
    while k < 16 {
        let v = input1.wrapping_mul(0x9e37_79b9).wrapping_add((k as u32).wrapping_mul(input2 | 1));
        unsafe { ptr::write_volatile(&mut l.bytes[k], (v >> (k & 7)) as u8) };
        k += 1;
    }

    // Whole-word reads of the byte buffer at a runtime word index.
    let j = (input2 & 3) as usize;
    let w0 = u32::from_le_bytes([l.bytes[4 * j], l.bytes[4 * j + 1], l.bytes[4 * j + 2], l.bytes[4 * j + 3]]);
    let w1 = u32::from_le_bytes(l.bytes[4 * ((j + 1) & 3)..4 * ((j + 1) & 3) + 4].try_into().unwrap());

    // Lanes read back in a permuted order.
    let mut acc = w0 ^ w1.rotate_left(11);
    let mut m = 0usize;
    while m < 16 {
        let lane = (m * 5 + j) & 15;
        acc = acc.rotate_left(3) ^ (l.bytes[lane] as u32);
        m += 1;
    }

    // Halfword stores at byte offsets 0/1/2/3 (and 4..7), runtime-selected.
    let off = (input1 & 3) as usize;
    let h0 = (input2 ^ 0x5a5a) as u16;
    let h1 = (input1 >> 7) as u16 ^ 0xa5a5;
    l.halves[off..off + 2].copy_from_slice(&h0.to_le_bytes());
    l.halves[off + 4..off + 6].copy_from_slice(&h1.to_le_bytes());
    l.halves[off + 9..off + 11].copy_from_slice(&h0.wrapping_add(h1).to_le_bytes());
    // Halfword read at the element-straddling offset 3 and at offset 1.
    let r3 = u16::from_le_bytes(l.halves[3..5].try_into().unwrap()) as u32;
    let r1 = u16::from_le_bytes(l.halves[off + 9..off + 11].try_into().unwrap()) as u32;
    acc = acc.wrapping_add(r3.rotate_left(5)).wrapping_add(r1.rotate_left(17));

    // Bytes out of a u64 stored to the frame, read at a runtime lane.
    let q = ((input1 as u64) << 32 | input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let qb = q.to_le_bytes();
    let lane = (input2 >> 3) as usize & 7;
    acc = acc.wrapping_add(qb[lane] as u32).wrapping_add((qb[lane ^ 5] as u32) << 8);

    // Negative i8/i16 lanes, sign-extended through 32- and 64-bit loads.
    let mut s = 0usize;
    while s < 8 {
        l.signed[s] = (input1 >> (s * 4)) as i8 ^ -1;
        l.shorts[s] = (input2 >> (s * 2)) as i16 ^ -256;
        s += 1;
    }
    let si = (input1 >> 5) as usize & 7;
    let a = l.signed[si] as i32;
    let b = l.signed[si ^ 3] as i64;
    let c = l.shorts[si] as i32;
    let d = l.shorts[si ^ 6] as i64;
    let wide = (b as u64).rotate_left(9) ^ (d as u64).rotate_left(21);
    acc = acc
        .wrapping_add(a as u32)
        .wrapping_add((c as u32).rotate_left(7))
        .wrapping_add(wide as u32)
        .wrapping_add((wide >> 32) as u32);

    // Walk every byte of the record so a clobbered neighbour lane shows.
    let mut i = 0usize;
    while i < 16 {
        acc = acc.rotate_left(1) ^ (l.bytes[i] as u32) ^ ((l.halves[i] as u32) << 8);
        i += 1;
    }
    acc
}

// Campaign 30 / W1: the same "load, opaque write, load again" straight line
// as `mo_cse_reload`, but every pair reads a DIFFERENT width of the same
// bytes — u8, i8 (`wasm.i32_load_8s`), u16 and i16 at an odd offset
// (element-straddling when the offset is 3), unaligned u32 and unaligned u64
// — and one cross-width pair reads u8 before the write and the whole
// containing u32 after it. The opaque write is an `#[inline(never)]` helper
// that pokes ONE byte at a runtime index, so the write may land inside or
// outside each load's footprint and LLVM can prove neither.
#[inline(never)]
fn poke(b: &mut [u8; 40], t: usize, v: u8) {
    b[t] = b[t].wrapping_add(v) | 1;
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = [0u8; 40];
    let mut k = 0usize;
    while k < 40 {
        b[k] = (input1.wrapping_mul(k as u32 + 7) ^ (input2 >> (k & 15))) as u8;
        k += 1;
    }
    // Probe offset, the 4-aligned word that contains it, and the poked byte.
    let o = (input2 & 31) as usize;
    let w = o & !3;
    let t = ((input2 >> 5) & 31) as usize;
    let v = (input1 >> 3) as u8;
    let mut acc = 0u32;

    // Same-width pairs, one straight line each.
    let a_u8 = b[o];
    poke(&mut b, t, v);
    let c_u8 = b[o];
    acc = acc.rotate_left(3) ^ (a_u8 as u32).wrapping_mul(0x0100_0193) ^ (c_u8 as u32);

    let a_i8 = b[o] as i8 as i32 as u32;
    poke(&mut b, t, v ^ 0x5a);
    let c_i8 = b[o] as i8 as i32 as u32;
    acc = acc.rotate_left(5) ^ a_i8 ^ c_i8.wrapping_mul(7);

    let a_u16 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u16) } as u32;
    poke(&mut b, t, v.wrapping_add(3));
    let c_u16 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u16) } as u32;
    acc = acc.rotate_left(7) ^ a_u16.wrapping_mul(31) ^ c_u16;

    let a_i16 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const i16) } as i32 as u32;
    poke(&mut b, t, v.rotate_left(2));
    let c_i16 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const i16) } as i32 as u32;
    acc = acc.rotate_left(11) ^ a_i16 ^ c_i16.wrapping_mul(13);

    let a_u32 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u32) };
    poke(&mut b, t, v ^ 0xa5);
    let c_u32 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u32) };
    acc = acc.rotate_left(13) ^ a_u32.wrapping_mul(3) ^ c_u32;

    let a_u64 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u64) };
    poke(&mut b, t, v.wrapping_mul(5));
    let c_u64 = unsafe { core::ptr::read_unaligned(b.as_ptr().add(o) as *const u64) };
    acc = acc.rotate_left(17) ^ (a_u64 as u32) ^ ((a_u64 >> 32) as u32).wrapping_mul(9);
    acc = acc.rotate_left(19) ^ ((c_u64 >> 32) as u32) ^ (c_u64 as u32).wrapping_mul(11);

    // Cross-width pair: narrow read before the write, wide read of the
    // containing word after it.
    let a_narrow = b[o] as u32;
    poke(&mut b, t, v | 0x40);
    let c_wide = unsafe { core::ptr::read_unaligned(b.as_ptr().add(w) as *const u32) };
    acc = acc.rotate_left(23) ^ a_narrow.wrapping_mul(0x9e37_79b9) ^ c_wide;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 40 {
        s = s.rotate_left(3).wrapping_add(b[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

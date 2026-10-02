// Campaign 30 / W4: `fill` and `ptr::write_bytes` over unaligned runtime
// ranges — every combination of (start % 4, length % 4) — plus a `memset`
// whose range covers a word that is afterwards read at u8, i8, u16 and u32
// width, and a `[u32]::fill` of an element range. `OpEmitter::memset` has no
// element fast path (it is a per-byte load/mask/or/store loop), so this is
// the shape where a byte-granular write has to leave the untouched lanes of
// the straddled elements alone; the reads at the two ends of the range are
// the check.
#[repr(C, align(4))]
struct Buf([u8; 96]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0u8; 96]);
    let mut k = 0usize;
    while k < 96 {
        b.0[k] = (input1.wrapping_mul(k as u32 + 19) ^ (input2 >> (k & 15))) as u8;
        k += 1;
    }
    let start = (input1 % 4) as usize + 4 * ((input2 >> 9) % 3) as usize; // 0..=11
    let len = (input2 % 12) as usize; // 0..=11
    let fv = (input1 >> 11) as u8;
    let mut acc = 0u32;

    // Slice fill over an unaligned runtime range.
    b.0[start..start + len].fill(fv);
    acc = acc.rotate_left(3) ^ (start as u32) ^ ((len as u32) << 8);

    // The lanes just outside the filled range must be untouched.
    let before = if start > 0 { b.0[start - 1] as u32 } else { 0xa5 };
    let after = b.0[start + len] as u32;
    acc = acc.rotate_left(5) ^ before.wrapping_mul(0x0100_0193) ^ after;

    // write_bytes over a second unaligned range, 32 bytes along.
    unsafe { core::ptr::write_bytes(b.0.as_mut_ptr().add(32 + start), fv ^ 0x3c, len) };
    let before2 = b.0[31 + start] as u32;
    let after2 = b.0[32 + start + len] as u32;
    acc = acc.rotate_left(7) ^ before2 ^ after2.wrapping_mul(31);

    // A memset covering a word that is then read at four widths.
    let w = 64 + 4 * ((input2 >> 4) % 4) as usize; // 4-aligned word start
    let mlen = ((input1 >> 5) % 7) as usize; // 0..=6 bytes from w + 1
    unsafe { core::ptr::write_bytes(b.0.as_mut_ptr().add(w + 1), fv.wrapping_add(7), mlen) };
    let r8 = b.0[w] as u32;
    let r8s = b.0[w + 1] as i8 as i32 as u32;
    let r16 = unsafe { core::ptr::read_unaligned(b.0.as_ptr().add(w + 1) as *const u16) } as u32;
    let r32 = unsafe { core::ptr::read_unaligned(b.0.as_ptr().add(w) as *const u32) };
    acc = acc.rotate_left(11) ^ r8 ^ r8s.wrapping_mul(3) ^ r16.wrapping_mul(13) ^ r32;

    // Element-typed fill of a u32 range.
    {
        let wp = unsafe { core::slice::from_raw_parts_mut(b.0.as_mut_ptr() as *mut u32, 24) };
        let ws = ((input2 >> 7) % 4) as usize + 12;
        let wl = ((input1 >> 9) % 5) as usize;
        wp[ws..ws + wl].fill(input1 ^ 0x9e37_79b9);
    }

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 96 {
        s = s.rotate_left(3).wrapping_add(b.0[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

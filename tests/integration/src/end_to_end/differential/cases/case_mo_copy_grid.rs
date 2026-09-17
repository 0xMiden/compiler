// Campaign 30 / W4: `copy_from_slice` and `ptr::copy_nonoverlapping` over
// DISJOINT halves of one 4-aligned buffer with runtime source and
// destination offsets covering all sixteen combinations of (src % 4, dst % 4)
// and lengths 0..=17, which is the full input space of the memcpy lowering's
// element fast path (`src % 4 == dst % 4 == count % 4 == 0` ->
// `memcopy_elements`) and of its byte fallback loop. A u32-element copy of
// the same region exercises the non-byte-pointer arm. The buffer is
// `#[repr(C, align(4))]` so both targets agree on which combinations are
// element-aligned; the whole buffer is hashed, so a copy of the wrong
// length, direction or offset changes the answer.
#[repr(C, align(4))]
struct Buf([u8; 128]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0u8; 128]);
    let mut k = 0usize;
    while k < 128 {
        b.0[k] = (input1.wrapping_mul(k as u32 + 13) ^ input2.rotate_left(k as u32 & 31)) as u8;
        k += 1;
    }
    let so = (input1 % 4) as usize; // source offset mod 4
    let doff = ((input1 >> 2) % 4) as usize; // destination offset mod 4
    let len = (input2 % 18) as usize; // 0..=17 bytes
    let mut acc = 0u32;

    // copy_from_slice between the two halves (source in the top half).
    {
        let (lo, hi) = b.0.split_at_mut(64);
        lo[doff..doff + len].copy_from_slice(&hi[so..so + len]);
    }
    acc = acc.rotate_left(3) ^ (len as u32) ^ ((so as u32) << 8) ^ ((doff as u32) << 16);

    // copy_nonoverlapping the other way, 32 bytes further along each half.
    unsafe {
        let base = b.0.as_mut_ptr();
        core::ptr::copy_nonoverlapping(base.add(32 + doff), base.add(96 + so), len);
    }

    // The same data through a u32-element copy (the non-byte-pointer arm of
    // `OpEmitter::memcpy`), element count derived from the byte length.
    let words = len / 4;
    unsafe {
        let wp = b.0.as_mut_ptr() as *mut u32;
        core::ptr::copy_nonoverlapping(wp.add(16), wp.add(24), words);
    }

    // Reads at both ends of the destination range, which the copy may or may
    // not have covered.
    let p0 = b.0[doff] as u32;
    let p1 = b.0[(doff + len).min(63)] as u32;
    acc = acc.rotate_left(5) ^ p0.wrapping_mul(0x0100_0193) ^ p1;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 128 {
        s = s.rotate_left(3).wrapping_add(b.0[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

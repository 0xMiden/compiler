// Spill slots live across the bulk frame operations that write the frame
// around them. One function defines an eight-u64 cluster, spills it across a
// loop, and inside that loop runs, on its own `[u8; 512]` locals: an explicit
// `core::ptr::write_bytes` with a runtime length (`memory.fill` -> `memset`),
// a short `copy_from_slice` below the 48-byte inline-copy threshold, a long
// one above it (`memory.copy` -> `memcpy`, element fast path when the runtime
// offsets happen to agree mod 4), and a 32-byte fill below the 64-byte
// bulk-fill threshold. The cluster is reloaded afterwards and the buffers are
// checksummed, so both a slot written by a frame fill and a frame byte
// written by a spill would show up.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut dst = [0u8; 512];
    let mut src = [0u8; 512];
    let mut tiny = [0u8; 32];
    let m = (input1 | 1) as u64;
    let q = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ q;
    let v1 = q.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ q.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    let mut i: u32 = 0;
    let mut k: usize = 0;
    while k < 512 {
        src[k] = (k as u8) ^ (input1 as u8);
        k = k.wrapping_add(1);
    }
    let mut acc = m ^ q.rotate_left(7);
    while i < (input2 % 3) + 2 {
        acc = acc.rotate_left(1)
            ^ (v0 ^ v1.rotate_left(3))
                .wrapping_add(v2 ^ v3.rotate_left(5))
                .wrapping_mul(v4 | 1)
            ^ (v5 ^ v6.rotate_left(7)).wrapping_sub(v7 ^ acc.rotate_left(9));
        // Runtime-length zero-ish fill of the frame while the cluster is spilled.
        let flen = 64 + ((acc >> 3) % 192) as usize;
        let foff = ((acc >> 11) % 64) as usize;
        unsafe {
            core::ptr::write_bytes(dst.as_mut_ptr().add(foff), (acc as u8) | 1, flen);
        }
        // Short copy (below the inline-copy threshold) and long copy (above it).
        let soff = ((acc >> 17) % 32) as usize;
        dst[256 + soff..256 + soff + 40].copy_from_slice(&src[soff..soff + 40]);
        let loff = ((acc >> 23) % 16) as usize;
        dst[280 + loff..280 + loff + 200].copy_from_slice(&src[loff..loff + 200]);
        // 32-byte fill, below the bulk-fill threshold.
        tiny = [(acc >> 40) as u8; 32];
        i = i.wrapping_add(1);
    }
    // Reloads after every bulk operation.
    let mut out = acc
        ^ v0.rotate_left(15)
        ^ v1.rotate_left(17)
        ^ v2.rotate_left(19)
        ^ v3.rotate_left(21)
        ^ v4.rotate_left(23)
        ^ v5.rotate_left(25)
        ^ v6.rotate_left(27)
        ^ v7.rotate_left(29);
    let mut sum: u32 = 0;
    let mut r: usize = 0;
    while r < 512 {
        sum = sum.rotate_left(1) ^ (dst[r] as u32) ^ ((src[r] as u32) << 8);
        r = r.wrapping_add(7);
    }
    let mut t: usize = 0;
    while t < 32 {
        sum = sum.wrapping_mul(31).wrapping_add(tiny[t] as u32);
        t = t.wrapping_add(1);
    }
    out ^= sum as u64;
    (out as u32) ^ ((out >> 32) as u32)
}

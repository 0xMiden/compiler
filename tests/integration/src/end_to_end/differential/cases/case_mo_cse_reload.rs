// Campaign 30 / W1: a load, an OPAQUE write, and the same load again, five
// times in a row with a different opaque-write kind each time and no branch
// between any pair. Both indexes come from the inputs, so LLVM cannot prove
// the write misses the loaded word and keeps both loads; in the HIR each
// pair is two `hir.load`s of the same address separated by a `hir.exec`
// (unknown effects), a `hir.store` (Write) or a `hir.memset` (Write), all of
// which must stop CSE from merging them. The five kinds are: an
// `#[inline(never)]` helper through `&mut u32`, one through `*mut u32`, one
// through `&mut [u32; 16]`, a `core::hint::black_box(&mut _)` mutation, and
// a `ptr::write_volatile`. Every pre- and post-write value is folded into
// the result, so a merged pair (b reusing a) changes the answer.
#[inline(never)]
fn write_ref(slot: &mut u32, v: u32) {
    *slot = slot.wrapping_mul(3).wrapping_add(v) | 1;
}

#[inline(never)]
fn write_ptr(base: *mut u32, idx: usize, v: u32) {
    unsafe {
        let p = base.add(idx);
        *p = (*p ^ v).wrapping_add(0x9e37_79b9);
    }
}

#[inline(never)]
fn write_arr(buf: &mut [u32; 16], idx: usize, v: u32) {
    buf[idx] = buf[idx].rotate_left(v & 31) ^ 0x5bf0_3635;
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u32; 16];
    let mut k = 0usize;
    while k < 16 {
        buf[k] = input1.wrapping_mul(k as u32 + 1) ^ input2.rotate_left(k as u32 & 31);
        k += 1;
    }
    let i = (input2 & 15) as usize;
    let j = ((input2 >> 4) & 15) as usize;
    let v = input1 | 1;
    let mut acc = 0u32;

    // Kind 1: `&mut u32` into an `#[inline(never)]` helper.
    let a1 = buf[i];
    write_ref(&mut buf[j], v);
    let b1 = buf[i];
    acc = acc.rotate_left(5) ^ a1.wrapping_mul(0x0100_0193) ^ b1;

    // Kind 2: a raw `*mut u32` derived from the array base.
    let a2 = buf[i];
    write_ptr(buf.as_mut_ptr(), j, v);
    let b2 = buf[i];
    acc = acc.rotate_left(7) ^ a2 ^ b2.wrapping_mul(0x0100_0193);

    // Kind 3: the whole array by `&mut`.
    let a3 = buf[i];
    write_arr(&mut buf, j, v);
    let b3 = buf[i];
    acc = acc.rotate_left(11) ^ a3.wrapping_mul(7) ^ b3;

    // Kind 4: `black_box(&mut _)` — no call in the HIR, just a store the
    // optimizer is not allowed to see through.
    let a4 = buf[i];
    {
        let r = core::hint::black_box(&mut buf);
        r[j] = r[j].wrapping_add(v).rotate_right(3);
    }
    let b4 = buf[i];
    acc = acc.rotate_left(13) ^ a4 ^ b4.wrapping_mul(31);

    // Kind 5: `write_volatile` of the neighbouring word (wraps within the
    // array), so the pinned grid can put the write on and off the loaded
    // word.
    let a5 = buf[i];
    unsafe {
        let p = buf.as_mut_ptr().add((j + 1) & 15);
        core::ptr::write_volatile(p, a5 ^ v);
    }
    let b5 = buf[i];
    acc = acc.rotate_left(17) ^ a5.wrapping_mul(17) ^ b5;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 16 {
        s = s.rotate_left(3).wrapping_add(buf[m] ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

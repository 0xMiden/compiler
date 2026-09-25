// Campaign 30 / W4: stores and loads immediately AROUND a bulk op. Each
// probe writes a byte inside the source range, runs the copy, then reads the
// same byte at the source and at the destination — so the copy must observe
// the preceding store (a `hir.store` before a `hir.memcpy` whose Read effect
// is on the source operand) and the following loads must observe the copy's
// write. The same sequence is repeated with a fill instead of a copy, with
// an element-aligned copy, and with `barrier()` between the bulk op and the
// read-back.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[repr(C, align(4))]
struct Buf([u8; 96]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut b = Buf([0u8; 96]);
    let mut k = 0usize;
    while k < 96 {
        b.0[k] = (input1.rotate_left(k as u32 * 7 & 31) ^ input2.wrapping_mul(k as u32 + 5)) as u8;
        k += 1;
    }
    let s = 48 + (input2 % 8) as usize; // source start 48..=55
    let d = (input1 % 8) as usize; // destination start 0..=7
    let len = 1 + (input2 >> 4) as usize % 12; // 1..=12
    let v = (input1 >> 7) as u8 | 1;
    let mut acc = 0u32;

    // Store into the source range, copy, read both ends.
    b.0[s] = v;
    b.0.copy_within(s..s + len, d);
    let r0 = b.0[d] as u32;
    let r1 = b.0[s] as u32;
    acc = acc.rotate_left(3) ^ r0.wrapping_mul(0x0100_0193) ^ r1;

    // Same with the last byte of the range and a call before the read-back.
    b.0[s + len - 1] = v.wrapping_add(0x5a);
    b.0.copy_within(s..s + len, d + 16);
    let g0 = barrier(acc);
    let r2 = b.0[d + 16 + len - 1] as u32;
    let r3 = b.0[s + len - 1] as u32;
    acc = acc.rotate_left(5) ^ r2 ^ r3.wrapping_mul(7) ^ g0;

    // Store, fill over the stored byte, read it back.
    b.0[d + 32] = v ^ 0x3c;
    b.0[d + 32..d + 32 + len].fill(v.rotate_left(3));
    let r4 = b.0[d + 32] as u32;
    let r5 = b.0[d + 32 + len] as u32;
    acc = acc.rotate_left(7) ^ r4.wrapping_mul(31) ^ r5;

    // Element-aligned copy (the memcpy fast path) with a store just before
    // and a wide read just after.
    let ws = 64 + 4 * ((input2 >> 8) % 3) as usize;
    let wl = 4 * ((input1 >> 9) % 4) as usize; // 0/4/8/12 bytes
    unsafe {
        core::ptr::write_unaligned(b.0.as_mut_ptr().add(ws) as *mut u32, input1 ^ 0x9e37_79b9);
    }
    b.0.copy_within(ws..ws + wl, 24);
    let r6 = unsafe { core::ptr::read_unaligned(b.0.as_ptr().add(24) as *const u32) };
    let r7 = unsafe { core::ptr::read_unaligned(b.0.as_ptr().add(ws) as *const u32) };
    acc = acc.rotate_left(11) ^ r6 ^ r7.wrapping_mul(13);

    // A copy whose source was just filled, read back through a byte view.
    b.0[40..40 + len].fill(v.wrapping_mul(5));
    b.0.copy_within(40..40 + len, d + 8);
    let r8 = b.0[d + 8] as u32;
    acc = acc.rotate_left(13) ^ r8.wrapping_mul(17);

    let mut sum = 0u32;
    let mut m = 0usize;
    while m < 96 {
        sum = sum.rotate_left(3).wrapping_add(b.0[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ sum
}

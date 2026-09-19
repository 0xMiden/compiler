// Campaign 30 / W1, the cross-object direction: the opaque write lands on a
// DIFFERENT object than the load, in both directions. Two loads of a
// frame-resident word straddle an atomic RMW / a `static mut` bump (so the
// two loads must agree — merging them is legal here, and a divergence would
// mean the static write reached the frame), and two loads of the statics
// straddle a store into the frame (same in reverse). A third pair puts a
// `write_volatile` of the frame between two reads of the SAME frame word, so
// the case also carries the aliasing direction where the values must differ.
// All mutable statics are restored before returning.
use core::sync::atomic::{AtomicU32, Ordering};

static COUNT: AtomicU32 = AtomicU32::new(0x0f0f_0f0f);
static mut PLAIN: u32 = 0x1234_5678;
static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn bump_atomic(v: u32) -> u32 {
    COUNT.fetch_add(v, Ordering::Relaxed)
}

#[inline(never)]
fn bump_plain(v: u32) -> u32 {
    unsafe {
        let p = core::ptr::addr_of_mut!(PLAIN);
        let old = *p;
        *p = old.rotate_left(9) ^ v;
        old
    }
}

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = [0u32; 16];
    let mut k = 0usize;
    while k < 16 {
        buf[k] = input1.wrapping_mul(k as u32 + 23) ^ input2.rotate_left(k as u32 & 31);
        k += 1;
    }
    let i = (input2 & 15) as usize;
    let v = input1 | 1;
    let mut acc = 0u32;

    // Frame load, atomic RMW on a static, frame load again: the two must
    // agree (the static is a different object).
    let a0 = buf[i];
    let prev = bump_atomic(v);
    let a1 = buf[i];
    acc = acc.rotate_left(3) ^ a0.wrapping_mul(0x0100_0193) ^ a1 ^ prev;

    // Frame load, `static mut` bump, frame load again.
    let a2 = buf[i];
    let prev2 = bump_plain(v.rotate_left(5));
    let a3 = buf[i];
    acc = acc.rotate_left(5) ^ a2 ^ a3.wrapping_mul(7) ^ prev2;

    // The reverse direction: read the statics, store into the frame, read
    // them again.
    let s0 = COUNT.load(Ordering::Relaxed);
    let t0 = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PLAIN)) };
    buf[i] = buf[i].rotate_left(11) ^ v;
    let s1 = COUNT.load(Ordering::Relaxed);
    let t1 = unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PLAIN)) };
    acc = acc.rotate_left(7) ^ s0 ^ s1.wrapping_mul(31) ^ t0 ^ t1.wrapping_mul(13);

    // The aliasing direction: a volatile write to the loaded word itself.
    let a4 = buf[i];
    unsafe { core::ptr::write_volatile(buf.as_mut_ptr().add(i), a4.rotate_left(17) ^ v) };
    let a5 = buf[i];
    acc = acc.rotate_left(11) ^ a4 ^ a5.wrapping_mul(17);

    // Two reads of the atomic around a pinned call that does not touch it.
    let s2 = COUNT.load(Ordering::Relaxed);
    let g = barrier(acc);
    let s3 = COUNT.load(Ordering::Relaxed);
    acc = acc.rotate_left(13) ^ s2 ^ s3.wrapping_mul(3) ^ g;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 16 {
        s = s.rotate_left(5).wrapping_add(buf[m] ^ (m as u32));
        m += 1;
    }

    // Restore both mutable statics.
    COUNT.store(0x0f0f_0f0f, Ordering::Relaxed);
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PLAIN), 0x1234_5678) };
    acc ^ s
}

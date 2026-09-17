// Campaign 30 / W2: one buffer seen through two raw-pointer views. The
// buffer is `#[repr(C, align(4))]` so `align_to::<u32>()` splits at the SAME
// place on both targets (a bare `[u8; N]` would split by the runtime address
// and diverge for layout reasons, not compiler ones). The prefix, middle and
// suffix are each written through their own view and read back through the
// byte view; then a u32 write is read as a byte and a byte write is read as
// a u32, in both orders, with and without `barrier()` between the store and
// the load. `barrier` is an `#[inline(never)]` helper pinned in place by a
// state-preserving `PIN.fetch_add(0)`.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[repr(C, align(4))]
struct Buf([u8; 48]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut buf = Buf([0u8; 48]);
    let mut k = 0usize;
    while k < 48 {
        buf.0[k] = (input1.wrapping_mul(k as u32 + 11) ^ (input2 >> (k & 15))) as u8;
        k += 1;
    }
    let off = (input2 & 7) as usize; // shifts the align_to split point
    let v = input1 | 1;
    let mut acc = 0u32;

    // Write through the three align_to views, read back as bytes.
    {
        let (pre, mid, suf) = unsafe { buf.0[off..off + 32].align_to_mut::<u32>() };
        let mut t = 0usize;
        while t < pre.len() {
            pre[t] = pre[t].wrapping_add(v as u8) | 1;
            t += 1;
        }
        t = 0;
        while t < mid.len() {
            mid[t] = mid[t].rotate_left(v & 31) ^ v;
            t += 1;
        }
        t = 0;
        while t < suf.len() {
            suf[t] ^= (v >> 8) as u8;
            t += 1;
        }
        acc = acc.rotate_left(3)
            ^ (pre.len() as u32)
            ^ ((mid.len() as u32) << 8)
            ^ ((suf.len() as u32) << 16);
    }
    let mut t = 0usize;
    while t < 32 {
        acc = acc.rotate_left(2) ^ (buf.0[off + t] as u32).wrapping_mul(t as u32 | 1);
        t += 1;
    }

    let wi = ((input2 >> 3) & 7) as usize; // u32 slot 0..7
    let bl = ((input1 >> 2) & 3) as usize; // byte lane inside it

    // Order 1: write the u32 view, read the byte view (no call between).
    unsafe {
        (buf.0.as_mut_ptr() as *mut u32).add(wi).write(v.wrapping_mul(0x9e37_79b9));
    }
    let r1 = buf.0[wi * 4 + bl] as u32;
    acc = acc.rotate_left(5) ^ r1.wrapping_mul(0x0100_0193);

    // Order 1 again, with the call between.
    unsafe {
        (buf.0.as_mut_ptr() as *mut u32).add(wi).write(v.wrapping_mul(0x85eb_ca6b));
    }
    let g1 = barrier(acc);
    let r2 = buf.0[wi * 4 + bl] as u32;
    acc = acc.rotate_left(7) ^ r2 ^ g1;

    // Order 2: write the byte view, read the u32 view (no call between).
    unsafe {
        *buf.0.as_mut_ptr().add(wi * 4 + bl) = (v >> 5) as u8;
    }
    let r3 = unsafe { (buf.0.as_ptr() as *const u32).add(wi).read() };
    acc = acc.rotate_left(11) ^ r3;

    // Order 2 again, with the call between.
    unsafe {
        *buf.0.as_mut_ptr().add(wi * 4 + bl) = (v >> 13) as u8;
    }
    let g2 = barrier(r3);
    let r4 = unsafe { (buf.0.as_ptr() as *const u32).add(wi).read() };
    acc = acc.rotate_left(13) ^ r4.wrapping_mul(31) ^ g2;

    // A byte written through the u8 view and read back signed through the
    // byte view, with the wide view read in between.
    unsafe {
        *buf.0.as_mut_ptr().add(wi * 4 + bl) = (v >> 21) as u8;
    }
    let r5 = unsafe { (buf.0.as_ptr() as *const u32).add(wi).read() };
    let r6 = buf.0[wi * 4 + bl] as i8 as i32 as u32;
    acc = acc.rotate_left(17) ^ r5.wrapping_mul(3) ^ r6;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 48 {
        s = s.rotate_left(3).wrapping_add(buf.0[m] as u32 ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

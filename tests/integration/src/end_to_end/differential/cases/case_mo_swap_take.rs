// Campaign 30 / W2: in-place element moves at RUNTIME indexes —
// `slice::swap`, `mem::replace`, `mem::take`, `mem::swap` through
// `split_at_mut`, and `ptr::swap` — each of which is a read-read-write-write
// quadruple on two addresses the optimizer cannot prove distinct (the two
// indexes may be equal, which is exactly the case `swap` must survive).
// Every probe is run once bare and once with `barrier()` between the move
// and the read-back, so the same shape is measured with and without an
// unknown-effect `hir.exec` in the middle.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 12];
    let mut k = 0usize;
    while k < 12 {
        a[k] = input1.rotate_left(k as u32 * 5 & 31) ^ input2.wrapping_mul(k as u32 + 2);
        k += 1;
    }
    let i = (input2 % 12) as usize;
    let j = ((input2 >> 4) % 12) as usize;
    let v = input1 | 1;
    let mut acc = 0u32;

    // slice::swap at two runtime indexes that may coincide.
    let p0 = a[i];
    a.swap(i, j);
    let q0 = a[i];
    let q1 = a[j];
    acc = acc.rotate_left(3) ^ p0.wrapping_mul(0x0100_0193) ^ q0 ^ q1.wrapping_mul(7);

    // Same swap with the pinned call between it and the read-back.
    a.swap(j, i);
    let g0 = barrier(acc);
    let q2 = a[i];
    let q3 = a[j];
    acc = acc.rotate_left(5) ^ q2 ^ q3.wrapping_mul(13) ^ g0;

    // mem::replace: returns the old value AND writes the new one.
    let old = core::mem::replace(&mut a[i], v);
    let q4 = a[i];
    acc = acc.rotate_left(7) ^ old.wrapping_mul(31) ^ q4;

    // mem::take: writes the zero and returns the old value, read back with
    // a call in between.
    let taken = core::mem::take(&mut a[j]);
    let g1 = barrier(taken);
    let q5 = a[j];
    acc = acc.rotate_left(11) ^ taken ^ q5.wrapping_mul(17) ^ g1;

    // mem::swap of two disjoint halves, indexes chosen at runtime.
    {
        let (lo, hi) = a.split_at_mut(6);
        core::mem::swap(&mut lo[i % 6], &mut hi[j % 6]);
    }
    let q6 = a[i % 6];
    let q7 = a[6 + j % 6];
    acc = acc.rotate_left(13) ^ q6.wrapping_mul(3) ^ q7;

    // ptr::swap of two possibly-identical addresses.
    unsafe {
        let base = a.as_mut_ptr();
        core::ptr::swap(base.add(i), base.add(j));
    }
    let q8 = a[i];
    let q9 = a[j];
    acc = acc.rotate_left(17) ^ q8 ^ q9.wrapping_mul(19);

    // A read of one element straddling a write to the other, no call.
    let before = a[i];
    a[j] = before.rotate_left(9) ^ v;
    let after = a[i];
    acc = acc.rotate_left(19) ^ before.wrapping_mul(11) ^ after;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 12 {
        s = s.rotate_left(5).wrapping_add(a[m] ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

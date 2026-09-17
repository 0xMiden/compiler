// Campaign 30 / W3: program order inside one expression. Four shapes whose
// answer depends on the loads and stores keeping their source order —
// `arr[i]` read before and after `arr[j] = v` with runtime i and j; a sum
// loop that writes a LATER index of the array it is reading; the same
// writing an EARLIER index; a helper that both returns the old value and
// writes a new one, called twice on the SAME slot inside one expression (the
// second call must observe the first call's write); and a `mem::replace`
// accumulator chain. `PIN.fetch_add(0)` pins the helper in place without
// changing any state.
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn bump_at(a: &mut [u32; 16], idx: usize, v: u32) -> u32 {
    let old = a[idx];
    a[idx] = old.rotate_left(7) ^ v ^ PIN.fetch_add(0, Ordering::Relaxed);
    old
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = [0u32; 16];
    let mut k = 0usize;
    while k < 16 {
        a[k] = input1.wrapping_mul(k as u32 + 3) ^ input2.rotate_left(k as u32 & 31);
        k += 1;
    }
    let i = (input2 & 15) as usize;
    let j = ((input2 >> 4) & 15) as usize;
    let v = input1 | 1;
    let mut acc = 0u32;

    // `a[i] + { a[j] = v; a[i] }` with the sequencing spelled out.
    let t0 = a[i];
    a[j] = v;
    let t1 = a[i];
    acc = acc.rotate_left(3) ^ t0.wrapping_add(t1).wrapping_mul(0x0100_0193);

    // Sum the array while writing five slots ahead of the read cursor.
    let mut s1 = 0u32;
    let mut m = 0usize;
    while m < 16 {
        s1 = s1.rotate_left(3) ^ a[m];
        a[(m + 5) & 15] = s1 ^ v;
        m += 1;
    }
    acc = acc.rotate_left(5) ^ s1;

    // Sum it again while writing five slots BEHIND the read cursor.
    let mut s2 = 0u32;
    m = 0;
    while m < 16 {
        s2 = s2.rotate_left(5).wrapping_add(a[m]);
        a[(m + 11) & 15] = s2.rotate_right(3) ^ v;
        m += 1;
    }
    acc = acc.rotate_left(7) ^ s2.wrapping_mul(31);

    // Two calls on the SAME slot in one expression: the second must see the
    // first one's write.
    let r0 = bump_at(&mut a, i, v).wrapping_add(bump_at(&mut a, i, v ^ 0x5a5a_5a5a));
    acc = acc.rotate_left(11) ^ r0;

    // Two calls on possibly-equal slots in one expression, mixed with a
    // direct read of each slot taken before the pair.
    let pre_i = a[i];
    let pre_j = a[j];
    let r1 = bump_at(&mut a, i, v.rotate_left(3))
        .wrapping_mul(3)
        .wrapping_add(bump_at(&mut a, j, v.rotate_left(9)));
    acc = acc.rotate_left(13) ^ r1 ^ pre_i.wrapping_mul(7) ^ pre_j;

    // A read whose value is consumed AFTER a store to the same array, in
    // one expression with the post-store read.
    let mixed = a[i].wrapping_add({
        a[j] = a[j].rotate_left(5) ^ v;
        a[i]
    });
    acc = acc.rotate_left(17) ^ mixed;

    // mem::replace accumulator: the replaced value must be the one before
    // the array read that computes the replacement.
    let mut accum = a[i];
    let next1 = accum.rotate_left(13) ^ a[j];
    let prev = core::mem::replace(&mut accum, next1);
    let next2 = accum.wrapping_mul(3) ^ a[(i + 1) & 15];
    let prev2 = core::mem::replace(&mut accum, next2);
    acc = acc.rotate_left(19) ^ prev ^ prev2.wrapping_mul(11) ^ accum;

    let mut s = 0u32;
    m = 0;
    while m < 16 {
        s = s.rotate_left(5).wrapping_add(a[m] ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

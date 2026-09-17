// Campaign 30 / W3: the `mo_seq_expr` ordering shapes carrying the
// campaign-20 spill freight, so the emitter's operand scheduler has to spill
// AROUND the loads and stores instead of keeping everything resident. Eight
// u64 values are defined before the memory probes, consumed inside them in
// one wide right-leaning expression (all live at a single program point) and
// used again afterwards; three masked rotate-count bands are used before and
// after the probes as well. If the scheduler's spill placement could move a
// load past a store, this is the shape that shows it.
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
    let m64 = (input1 | 1) as u64;
    let n64 = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let v0 = m64.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ n64;
    let v1 = n64.wrapping_mul(0xbf58_476d_1ce4_e5b9) ^ m64.rotate_left(11);
    let v2 = v0.rotate_left(17) ^ n64.wrapping_mul(0x94d0_49bb_1331_11eb);
    let v3 = v1.rotate_left(23) ^ m64.wrapping_mul(0xd6e8_feb8_6659_fd93);
    let v4 = v2.wrapping_add(v0.rotate_left(29)) ^ 0xa076_1d64_78bd_642f;
    let v5 = v3.wrapping_sub(v1.rotate_left(31)) ^ 0xe703_7ed1_a0b4_28db;
    let v6 = v4.rotate_left(9) ^ v2.wrapping_mul(0x8ebc_6af0_9c88_c6e3);
    let v7 = v5.rotate_left(13) ^ v3.wrapping_mul(0x5895_58cb_3521_e49d);
    // Three masked rotate-count bands, used before and after the probes.
    let b0 = (input1 >> 2) & 31;
    let b1 = (input2 >> 7) & 31;
    let b2 = (input1 ^ input2) & 31;

    let mut a = [0u32; 16];
    let mut k = 0usize;
    while k < 16 {
        a[k] = input1.rotate_left(b0) ^ input2.rotate_left(b1).wrapping_mul(k as u32 + 3);
        k += 1;
    }
    let i = (input2 & 15) as usize;
    let j = ((input2 >> 4) & 15) as usize;
    let v = input1.rotate_left(b2) | 1;
    let mut acc = input1.rotate_left(b0) ^ input2.rotate_left(b1);

    // `a[i]` read, store to `a[j]`, read again, with the whole cluster live.
    let t0 = a[i];
    a[j] = v;
    let t1 = a[i];
    acc = acc.rotate_left(b0) ^ t0.wrapping_add(t1).wrapping_mul(0x0100_0193);

    // One wide right-leaning expression over all eight cluster values.
    let wide = (v1 ^ v7.rotate_left(1))
        .wrapping_add(v2 ^ v6.rotate_left(3))
        .wrapping_mul(v3 | 1)
        ^ v5.rotate_left(5)
        ^ (v0 ^ v4.rotate_left(7)).wrapping_sub(v2 ^ v1.rotate_left(9));
    a[(i + 1) & 15] = (wide as u32) ^ ((wide >> 32) as u32);

    // Sum while writing ahead of the cursor, bands in the loop body.
    let mut s1 = 0u32;
    let mut p = 0usize;
    while p < 16 {
        s1 = s1.rotate_left(b1 & 31) ^ a[p];
        a[(p + 5) & 15] = s1.rotate_left(b2 & 31) ^ v;
        p += 1;
    }
    acc = acc.rotate_left(b1) ^ s1;

    // Two calls on the same slot inside one expression, still under freight.
    let r0 = bump_at(&mut a, i, v).wrapping_add(bump_at(&mut a, i, v ^ 0x5a5a_5a5a));
    acc = acc.rotate_left(b2) ^ r0;

    // A read consumed after a store to the same array, in one expression.
    let mixed = a[i].wrapping_add({
        a[j] = a[j].rotate_left(b0 & 31) ^ v;
        a[i]
    });
    acc = acc.rotate_left(b0) ^ mixed.wrapping_mul(31);

    // The cluster is live again here, after every memory op.
    let tail = v0
        .rotate_left(3)
        ^ v1.rotate_left(7)
        ^ v2.rotate_left(11)
        ^ v3.rotate_left(17)
        ^ v4.rotate_left(23)
        ^ v5.rotate_left(29)
        ^ v6.rotate_left(31)
        ^ v7.rotate_left(37);
    acc = acc.rotate_left(b1) ^ (tail as u32) ^ ((tail >> 32) as u32);

    let mut s = 0u32;
    p = 0;
    while p < 16 {
        s = s.rotate_left(5).wrapping_add(a[p] ^ (p as u32));
        p += 1;
    }
    acc ^ s
}

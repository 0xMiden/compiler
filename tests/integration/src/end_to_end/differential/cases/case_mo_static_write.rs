// Campaign 30 / W6: reads of `static` data after in-place writes to its
// neighbours. An immutable `.rodata` table is read at a constant index and
// at a runtime index around an `#[inline(never)]` helper that writes its
// `static mut` `.data` twin (same size, adjacent segment); the twin is then
// written and read back in ONE block with and without an opaque call in
// between; and an `AtomicU32` `swap` is folded into the result. Every mutable
// static is restored to its initial contents before returning, because the
// native cdylib is loaded once and reused for every input pair.
use core::sync::atomic::{AtomicU32, Ordering};

static TABLE: [u32; 16] = [
    0x0000_0001,
    0x0000_0102,
    0x0001_0203,
    0x0102_0304,
    0x1020_3040,
    0x2030_4050,
    0x3040_5060,
    0x4050_6070,
    0x5060_7080,
    0x6070_8090,
    0x7080_90a0,
    0x8090_a0b0,
    0x90a0_b0c0,
    0xa0b0_c0d0,
    0xb0c0_d0e0,
    0xc0d0_e0f0,
];

static INIT: [u32; 16] = [
    0x1111_1111,
    0x2222_2222,
    0x3333_3333,
    0x4444_4444,
    0x5555_5555,
    0x6666_6666,
    0x7777_7777,
    0x8888_8888,
    0x9999_9999,
    0xaaaa_aaaa,
    0xbbbb_bbbb,
    0xcccc_cccc,
    0xdddd_dddd,
    0xeeee_eeee,
    0xffff_ffff,
    0x0123_4567,
];

static mut MIRROR: [u32; 16] = [
    0x1111_1111,
    0x2222_2222,
    0x3333_3333,
    0x4444_4444,
    0x5555_5555,
    0x6666_6666,
    0x7777_7777,
    0x8888_8888,
    0x9999_9999,
    0xaaaa_aaaa,
    0xbbbb_bbbb,
    0xcccc_cccc,
    0xdddd_dddd,
    0xeeee_eeee,
    0xffff_ffff,
    0x0123_4567,
];

static COUNTER: AtomicU32 = AtomicU32::new(0x5a5a_5a5a);
static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn poke_mirror(idx: usize, v: u32) -> u32 {
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MIRROR);
        let old = m[idx];
        m[idx] = old.rotate_left(5) ^ v;
        old
    }
}

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let i = (input2 & 15) as usize;
    let v = input1 | 1;
    let mut acc = 0u32;

    // Constant-index and runtime-index reads of the IMMUTABLE table, before
    // and after a write to its mutable twin.
    let c0 = TABLE[5];
    let r0 = TABLE[i];
    let old = poke_mirror(i, v);
    let c1 = TABLE[5];
    let r1 = TABLE[i];
    acc = acc.rotate_left(3) ^ c0 ^ c1.wrapping_mul(7) ^ r0 ^ r1.wrapping_mul(0x0100_0193);
    acc = acc.rotate_left(5) ^ old;

    // The mutable static written and read back in one block, no call.
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MIRROR);
        let a0 = m[i];
        m[i] = a0.rotate_left(7) ^ v;
        let a1 = m[i];
        acc = acc.rotate_left(7) ^ a0 ^ a1.wrapping_mul(31);
    }

    // Same, with a pinned opaque call between the write and the read.
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MIRROR);
        let a2 = m[i];
        m[i] = a2.wrapping_mul(3) ^ v.rotate_left(11);
        let g = barrier(a2);
        let a3 = m[i];
        acc = acc.rotate_left(11) ^ a3 ^ g;
    }

    // A write to the mutable twin followed by the immutable read at the SAME
    // index: the two statics are distinct segments and must not alias.
    let before = TABLE[i];
    let _ = poke_mirror((i + 1) & 15, v.rotate_left(13));
    let after = TABLE[i];
    acc = acc.rotate_left(13) ^ before.wrapping_mul(13) ^ after;

    // AtomicU32 swap folded into the result, then restored.
    let sw = COUNTER.swap(v, Ordering::Relaxed);
    acc = acc.rotate_left(17) ^ sw;
    COUNTER.store(0x5a5a_5a5a, Ordering::Relaxed);

    // Fold the whole mirror, then restore it from the immutable initializer.
    unsafe {
        let m = &mut *core::ptr::addr_of_mut!(MIRROR);
        let mut k = 0usize;
        while k < 16 {
            acc = acc.rotate_left(3).wrapping_add(m[k] ^ (k as u32));
            k += 1;
        }
        k = 0;
        while k < 16 {
            m[k] = INIT[k];
            k += 1;
        }
    }
    acc
}

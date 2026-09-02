// Atomic and volatile memory traffic (single-threaded, but the ops lower
// through different LLVM paths): `AtomicU32`/`AtomicU64` load/store/
// fetch_add/fetch_xor/swap/compare_exchange in sequence on `.data`
// statics (restored before returning: the native cdylib is reused across
// all proptest inputs), an `AtomicU8` lane inside a word beside an
// `AtomicU16`, and `read_volatile`/`write_volatile` of u8/u16/u32 values at
// runtime byte offsets of a frame buffer (volatile keeps every sub-word
// access as its own load/store). The buffer is hashed whole at the end.
use core::{
    ptr,
    sync::atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering::Relaxed},
};

static A: AtomicU32 = AtomicU32::new(0x1357_9bdf);
static B: AtomicU64 = AtomicU64::new(0x0f1e_2d3c_4b5a_6978);
static C: AtomicU8 = AtomicU8::new(0x42);
static D: AtomicU16 = AtomicU16::new(0xbeef);

#[repr(C, align(4))]
struct Frame([u8; 32]);

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a0 = A.load(Relaxed);
    let b0 = B.load(Relaxed);
    let c0 = C.load(Relaxed);
    let d0 = D.load(Relaxed);

    let p1 = A.fetch_add(input1, Relaxed);
    let p2 = A.fetch_xor(input2.rotate_left(5), Relaxed);
    let p3 = A.swap(p1 ^ p2, Relaxed);
    let cx = match A.compare_exchange(p1 ^ p2, input1.wrapping_mul(3), Relaxed, Relaxed) {
        Ok(v) => v,
        Err(v) => v.rotate_left(1),
    };
    let cf = match A.compare_exchange(input2, 7, Relaxed, Relaxed) {
        Ok(v) => v | 1,
        Err(v) => v & !1,
    };
    let q1 = B.fetch_add((input2 as u64) << 20 | input1 as u64, Relaxed);
    let q2 = B.fetch_xor(0x9e37_79b9_7f4a_7c15u64.wrapping_mul(input1 as u64 | 1), Relaxed);
    let q3 = B.swap(q1.rotate_left(input2 & 63), Relaxed);
    let qc = match B.compare_exchange(q1.rotate_left(input2 & 63), q2 ^ q3, Relaxed, Relaxed) {
        Ok(v) => v,
        Err(v) => !v,
    };
    let r1 = C.fetch_add(input1 as u8, Relaxed);
    let r2 = D.fetch_xor(input2 as u16, Relaxed);
    let r3 = C.swap(r2 as u8, Relaxed);
    let a1 = A.load(Relaxed);
    let b1 = B.load(Relaxed);
    let c1 = C.load(Relaxed) as u32;
    let d1 = D.load(Relaxed) as u32;

    A.store(a0, Relaxed);
    B.store(b0, Relaxed);
    C.store(c0, Relaxed);
    D.store(d0, Relaxed);

    let mut acc = p3 ^ cx.rotate_left(3) ^ cf.rotate_left(7) ^ a1;
    acc = acc.wrapping_add(qc as u32).wrapping_add((qc >> 32) as u32);
    acc = acc.wrapping_add(b1 as u32 ^ (b1 >> 32) as u32);
    acc = acc.wrapping_add((r1 as u32) | ((r3 as u32) << 8) | (c1 << 16)).wrapping_add(d1);

    // Volatile sub-word traffic at runtime offsets of a frame buffer.
    let mut f = Frame([0; 32]);
    let o = (input1 & 3) as usize;
    let base = f.0.as_mut_ptr();
    unsafe {
        let mut k = 0usize;
        while k < 32 {
            ptr::write_volatile(base.add(k), (acc >> (k & 7)) as u8);
            k += 1;
        }
        ptr::write_volatile(base.add(o + 4), input2 as u8);
        ptr::write_unaligned(base.add(o + 9) as *mut u16, input1 as u16 ^ 0x0f0f);
        ptr::write_unaligned(base.add(o + 13) as *mut u32, acc.rotate_left(o as u32 * 8));
        let v8 = ptr::read_volatile(base.add(o + 4)) as u32;
        let v16 = ptr::read_unaligned(base.add(o + 9) as *const u16) as u32;
        let v32 = ptr::read_unaligned(base.add(o + 13) as *const u32);
        let n16 = ptr::read_unaligned(base.add(o + 11) as *const u16) as u32;
        acc = acc.wrapping_add(v8).wrapping_add(v16 << 8).wrapping_add(v32.rotate_left(5)).wrapping_add(n16 << 3);
        k = 0;
        while k < 32 {
            acc = acc.rotate_left(1) ^ (ptr::read_volatile(base.add(k)) as u32);
            k += 1;
        }
    }
    acc
}

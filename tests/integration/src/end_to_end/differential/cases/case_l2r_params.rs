// The reliable producer of promotable slots: the frontend gives EVERY
// function parameter an unconditional `hir.store_local` at entry, so a
// parameter read exactly once in straight-line entry-block code is a
// single-store/single-load local with no control flow in between — the one
// shape Local2Reg promotes. Four `#[inline(never)]` helpers with three to
// five such parameters each maximise the promotable set, and
// `l2r_unused_arg` adds a stored-but-never-loaded parameter for the
// dead-store-erasure arm. The whole point is that erasing those stores is
// invisible: the answer must be the same at every guest debug level.

use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn five(a: u32, b: u32, c: u32, d: u32, e: u32) -> u32 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    a.rotate_left(3) ^ b.wrapping_mul(0x0100_0193) ^ c.wrapping_sub(d) ^ (e >> 5) ^ p
}

#[inline(never)]
fn four64(a: u64, b: u64, c: u32, d: u32) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    a.rotate_left(11) ^ b.wrapping_add(c as u64) ^ ((d as u64) << 19) ^ p
}

#[inline(never)]
fn three(a: u32, b: u32, c: u32) -> u32 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    (a ^ b).wrapping_add(c.rotate_right(13)) ^ p
}

// `unused` is stored at entry and never loaded: the dead-store arm.
// `#[no_mangle]` gives the helper external linkage so LLVM's dead-argument
// elimination cannot drop the parameter before it reaches wasm.
#[inline(never)]
#[unsafe(no_mangle)]
extern "C" fn l2r_unused_arg(a: u32, unused: u32, c: u32) -> u32 {
    let _ = unused;
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    a.wrapping_add(c).rotate_left(7) ^ p
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = five(input1, input2, input1 ^ input2, input1 | 1, input2 | 2);
    let b = four64(
        ((input1 as u64) << 32) | input2 as u64,
        (input2 as u64) | 3,
        a,
        input1,
    );
    let c = three(a, (b as u32) ^ input1, (b >> 32) as u32);
    let d = l2r_unused_arg(c, a ^ input2, input1.rotate_left(5));
    c.wrapping_add(d) ^ (b as u32) ^ ((b >> 32) as u32)
}

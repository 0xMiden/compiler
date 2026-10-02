// A local defined before a call and read only after it. Local2Reg's second
// heuristic rejects a promotion whose store and load are separated by any op
// implementing `BranchOpInterface`, `RegionBranchOpInterface` or
// `CallOpInterface`, so a slot that is live across a call must keep its
// store/load pair no matter the debug level, while the single-use temporaries
// feeding the call arguments stay promotable. Three `#[inline(never)]`
// helpers with an opaque `fetch_add(0)` keep the calls in place (LLVM sinks
// readnone helpers to their use, which would destroy the liveness this case
// is about).

use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn mix_a(x: u32, y: u32) -> u32 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    x.rotate_left(y & 31).wrapping_add(y ^ 0x5bf0_3635) ^ p
}

#[inline(never)]
fn mix_b(x: u64, y: u32) -> u64 {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    x.rotate_right((y & 63) as u32) ^ x.wrapping_add(y as u64) ^ p
}

#[inline(never)]
fn mix_c(x: u32) -> u32 {
    let p = PIN.fetch_add(0, Ordering::Relaxed);
    (x ^ (x >> 13)).wrapping_mul(0x2545_f491) ^ p
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Live across all three calls; read once, at the very end.
    let across = input1.rotate_left(9) ^ input2.wrapping_mul(0x27d4_eb2f);
    // Live across two calls.
    let across64 = ((input1 as u64) << 24) ^ ((input2 as u64) | 7);

    let a = mix_a(input1, input2);
    let b = mix_b(across64, a);
    let c = mix_c(a ^ (b as u32));

    across
        .wrapping_add(c)
        .rotate_left(a & 31)
        .wrapping_sub((across64 as u32) ^ ((b >> 32) as u32))
}

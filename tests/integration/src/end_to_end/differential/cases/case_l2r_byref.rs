// Two locals side by side: `kept` is an ordinary value whose address is never
// taken (a wasm local, so a Local2Reg candidate), while `slot` is passed by
// `&mut` to an `#[inline(never)]` helper. Taking the address forces `slot`
// out of the wasm local space and into the guest's shadow stack in linear
// memory, so it never becomes a `hir.store_local` at all and cannot be
// promoted under any debug level. The helper carries an opaque
// `fetch_add(0)` so LLVM can neither inline the call away nor sink it, which
// also keeps `kept` live ACROSS the call. Semantics must be identical with
// and without guest DWARF.

use core::sync::atomic::{AtomicU32, Ordering};

// Never changes state: read as an unfoldable zero, safe across the reused
// native cdylib invocations.
static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn bump(slot: &mut u64, k: u32) {
    let p = PIN.fetch_add(0, Ordering::Relaxed) as u64;
    *slot = slot.rotate_left(k & 63) ^ (*slot >> 7) ^ p;
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let kept = input1.wrapping_mul(0x0100_0193) ^ 0x811c_9dc5;
    let mut slot: u64 = ((input2 as u64) << 32) | (input1 as u64) | 1;

    bump(&mut slot, input2);
    bump(&mut slot, kept);

    let tail = slot ^ (kept as u64);
    (tail as u32) ^ ((tail >> 32) as u32) ^ kept.rotate_left(input2 & 31)
}

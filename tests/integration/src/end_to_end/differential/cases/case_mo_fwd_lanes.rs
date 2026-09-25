// Campaign 30 / W2: store-then-load forwarding at every lane and width of a
// 4-aligned `[u32; 9]` read back through a byte view. Each probe stores at
// one width and immediately loads at another — u32 store then u8 lane k, the
// four u8 lane stores then a whole u32 load, a u16 store at byte offset
// 0/1/2/3 (3 straddles the element boundary) then u32 loads of BOTH touched
// words, and a u32 store followed by an unaligned u32 load one byte away.
// Every probe is run twice: once as a bare straight line, and once with
// `barrier()` between the store and the load — an `#[inline(never)]` helper
// whose `PIN.fetch_add(0)` is a state-preserving atomic side effect, so the
// call cannot be sunk, folded or reordered (and the static is unchanged, so
// the case stays deterministic across the reused native cdylib).
use core::sync::atomic::{AtomicU32, Ordering};

static PIN: AtomicU32 = AtomicU32::new(0);

#[inline(never)]
fn barrier(x: u32) -> u32 {
    x ^ PIN.fetch_add(0, Ordering::Relaxed)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut w = [0u32; 9];
    let mut n = 0usize;
    while n < 9 {
        w[n] = input1.wrapping_mul(n as u32 + 5) ^ input2.rotate_left(n as u32 * 3 & 31);
        n += 1;
    }
    let i = (input2 & 7) as usize; // word index, leaves one word of slack
    let k = ((input2 >> 3) & 3) as usize; // byte lane inside the word
    let v = input1 | 0x0101_0101;
    let mut acc = 0u32;

    // u32 store, then the u8 lane, with no call in between.
    w[i] = v;
    let l0 = unsafe { *(w.as_ptr() as *const u8).add(i * 4 + k) } as u32;
    acc = acc.rotate_left(3) ^ l0.wrapping_mul(0x0100_0193);

    // Same, with a pinned opaque call between the store and the load.
    w[i] = v.rotate_left(8);
    let g0 = barrier(acc);
    let l1 = unsafe { *(w.as_ptr() as *const u8).add(i * 4 + k) } as u32;
    acc = acc.rotate_left(5) ^ l1 ^ g0;

    // Signed lane read (`wasm.i32_load_8s`) of a just-stored word.
    w[i] = v.rotate_left(16);
    let l2 = unsafe { *((w.as_ptr() as *const u8).add(i * 4 + k) as *const i8) } as i32 as u32;
    acc = acc.rotate_left(7) ^ l2.wrapping_mul(7);

    // Four u8 lane stores, then the whole word back.
    unsafe {
        let bp = w.as_mut_ptr() as *mut u8;
        *bp.add(i * 4) = v as u8;
        *bp.add(i * 4 + 1) = (v >> 8) as u8;
        *bp.add(i * 4 + 2) = (v >> 16) as u8;
        *bp.add(i * 4 + 3) = (v >> 24) as u8;
    }
    let l3 = w[i];
    acc = acc.rotate_left(11) ^ l3;

    // The same four lane stores with a call before the wide read.
    unsafe {
        let bp = w.as_mut_ptr() as *mut u8;
        *bp.add(i * 4) = (v >> 1) as u8;
        *bp.add(i * 4 + 1) = (v >> 9) as u8;
        *bp.add(i * 4 + 2) = (v >> 17) as u8;
        *bp.add(i * 4 + 3) = (v >> 25) as u8;
    }
    let g1 = barrier(l3);
    let l4 = w[i];
    acc = acc.rotate_left(13) ^ l4.wrapping_mul(31) ^ g1;

    // u16 store at byte offset k (odd = unaligned, 3 = element-straddling),
    // then both touched words read whole.
    unsafe {
        core::ptr::write_unaligned(
            (w.as_mut_ptr() as *mut u8).add(i * 4 + k) as *mut u16,
            (v >> 3) as u16,
        );
    }
    let l5 = w[i];
    let l6 = w[i + 1];
    acc = acc.rotate_left(17) ^ l5 ^ l6.wrapping_mul(13);

    // Same u16 store with a call between, and the two words read in the
    // opposite order.
    unsafe {
        core::ptr::write_unaligned(
            (w.as_mut_ptr() as *mut u8).add(i * 4 + k) as *mut u16,
            (v >> 11) as u16,
        );
    }
    let g2 = barrier(l6);
    let l7 = w[i + 1];
    let l8 = w[i];
    acc = acc.rotate_left(19) ^ l7.wrapping_mul(3) ^ l8 ^ g2;

    // u32 store, then an unaligned u32 load starting one byte later.
    w[i] = v.wrapping_mul(0x9e37_79b9);
    let l9 = unsafe {
        core::ptr::read_unaligned((w.as_ptr() as *const u8).add(i * 4 + 1) as *const u32)
    };
    acc = acc.rotate_left(23) ^ l9;

    let mut s = 0u32;
    let mut m = 0usize;
    while m < 9 {
        s = s.rotate_left(5).wrapping_add(w[m] ^ (m as u32));
        m += 1;
    }
    acc ^ s
}

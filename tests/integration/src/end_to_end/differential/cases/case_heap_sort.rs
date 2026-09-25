// W3 — sorting heap buffers. `core`'s STABLE sorts are unusable on this
// target (see `heap_sort_stable_nolink`: `core::slice::sort::stable::tiny::
// mergesort` is recursive and the assembler rejects the call graph at every
// optimization level), so the stable half is a HAND-WRITTEN bottom-up merge
// sort over a scratch `Vec` — which keeps the shape that mattered: a sorting
// algorithm that ALLOCATES a second buffer and copies runs between the two.
//
// `sort_unstable*` is the non-recursive `heapsort` the compiler's
// `-Zbuild-std-features=optimize_for_size` build of `core` provides, and
// allocates nothing.
//
// CAUTION, verified here: the two sides link DIFFERENT unstable sorts. The
// guest wasm contains only `core::slice::sort::unstable::heapsort::heapsort`,
// while the native `cdylib` (host sysroot `core`, no `optimize_for_size`)
// contains `core::slice::sort::unstable::ipnsort` with `median3_rec`,
// `small_sort_network` and `insertion_sort_shift_left`. An unstable sort does
// not specify the order of EQUAL keys, so any use whose answer can see that
// order is a FALSE divergence: `sort_unstable_by_key(|e| e >> 40)` on the
// `Vec<u64>` below diverged at (983633457, 2147483648) for exactly this
// reason. The rule this case follows: an unstable sort's key must be
// INJECTIVE over the element (here `e.rotate_left(17)`, a bijection), or the
// element must BE the key (`sort_unstable()`, where ties are indistinguishable).
//
// Stability is checked directly: the `Vec<u64>` packs a KEY in the high half
// and the original POSITION in the low half, is sorted by the key alone, and
// the positions are hashed in the sorted order — so equal keys staying in
// their original relative order is part of the answer, not just the multiset.
// Duplicates are forced by masking the key to 5 bits. Lengths run 0..40,
// covering the empty, one-element and multi-pass cases.
//
// Bulk-op evidence: 1 `memory.copy`, 0 `memory.fill`, no libcall.
// Arena: 64 KiB.

extern crate alloc;

use alloc::vec::Vec;
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
};

const ARENA_SIZE: usize = 1 << 16;

#[repr(align(16))]
struct Arena(UnsafeCell<[u8; ARENA_SIZE]>);
unsafe impl Sync for Arena {}
static ARENA: Arena = Arena(UnsafeCell::new([0; ARENA_SIZE]));
static mut NEXT: usize = 0;

struct Bump;

unsafe impl GlobalAlloc for Bump {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let base = ARENA.0.get() as *mut u8;
        let start = (unsafe { NEXT } + layout.align() - 1) & !(layout.align() - 1);
        let end = start + layout.size();
        if end > ARENA_SIZE {
            return core::ptr::null_mut();
        }
        unsafe { NEXT = end };
        unsafe { base.add(start) }
    }

    unsafe fn dealloc(&self, _p: *mut u8, _l: Layout) {}
}

#[global_allocator]
static GLOBAL: Bump = Bump;

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(2654435761).rotate_left(9)
}

// Bottom-up merge sort by the high 32 bits, into and out of a scratch
// buffer allocated once. Iterative on purpose: a recursive merge sort is a
// call-graph cycle the assembler rejects.
fn merge_sort_by_key(data: &mut Vec<u64>) {
    let len = data.len();
    if len < 2 {
        return;
    }
    let mut scratch: Vec<u64> = Vec::with_capacity(len);
    for i in 0..len {
        scratch.push(data[i]);
    }
    let mut width = 1usize;
    while width < len {
        let mut lo = 0usize;
        while lo < len {
            let mid = if lo + width < len { lo + width } else { len };
            let hi = if lo + 2 * width < len { lo + 2 * width } else { len };
            let mut a = lo;
            let mut b = mid;
            let mut out = lo;
            while a < mid || b < hi {
                let take_left = if a >= mid {
                    false
                } else if b >= hi {
                    true
                } else {
                    // `<=` keeps equal keys in their original order.
                    (data[a] >> 32) <= (data[b] >> 32)
                };
                if take_left {
                    scratch[out] = data[a];
                    a += 1;
                } else {
                    scratch[out] = data[b];
                    b += 1;
                }
                out += 1;
            }
            lo += 2 * width;
        }
        for i in 0..len {
            data[i] = scratch[i];
        }
        width *= 2;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 41) as usize;
    let mut x = input2 | 1;
    let mut h: u32 = 0x1357_9bdf;

    // Stable merge sort of (key, position) pairs packed into u64s.
    let mut pairs: Vec<u64> = Vec::new();
    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        let key = (x >> 22) & 31;
        pairs.push(((key as u64) << 32) | (i as u64));
    }
    merge_sort_by_key(&mut pairs);
    for i in 0..pairs.len() {
        h = mix(h, (pairs[i] >> 32) as u32);
        h = mix(h, pairs[i] as u32);
    }

    // Unstable sort (allocation-free heapsort) of a Vec<u32> with duplicates.
    let mut flat: Vec<u32> = Vec::new();
    for i in 0..n {
        flat.push(((x >> (i & 15)) & 63) ^ (i as u32 & 3));
    }
    flat.sort_unstable();
    for i in 0..flat.len() {
        h = mix(h, flat[i]).rotate_left(1);
    }

    // Unstable sort by a key closure, on 64-bit elements.
    let mut wide: Vec<u64> = Vec::new();
    for i in 0..n {
        wide.push(((x.rotate_left((i & 31) as u32) as u64) << 24) | (i as u64));
    }
    wide.sort_unstable_by_key(|e| e.rotate_left(17));
    for i in 0..wide.len() {
        h = mix(h, wide[i] as u32) ^ ((wide[i] >> 32) as u32);
    }

    // A descending sort through a comparator closure.
    let mut down: Vec<u32> = Vec::new();
    for i in 0..n {
        down.push(x.rotate_left((i & 31) as u32) >> 24);
    }
    down.sort_unstable_by(|a, b| b.cmp(a));
    for i in 0..down.len() {
        h = mix(h, down[i]);
    }

    h.wrapping_add(pairs.len() as u32).wrapping_add((flat.len() as u32) << 8)
}

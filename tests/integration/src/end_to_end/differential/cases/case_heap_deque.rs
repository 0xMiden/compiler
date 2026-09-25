// W3 — `VecDeque<u32>`: a ring buffer whose head index wraps. The push
// sequence is chosen so the deque is WRAPPED (head near the end of the
// buffer, tail at the start) when it grows, which is the interesting realloc
// path: after the buffer is reallocated the two live segments have to be
// re-joined, which `handle_capacity_increase` does with an extra copy on top
// of the realloc copy.
//
// Wrap-around index arithmetic (`to_physical_idx`, `wrap_add`) is the other
// target: every `get`/`pop` maps a logical index to a physical one modulo a
// capacity that is not a compile-time constant here.
//
// Results are a rolling hash of the popped values in order, plus the lengths:
// nothing capacity-dependent escapes (`capacity()` differs by allocator, and
// a `usize` is 64 bits natively and 32 on wasm).
//
// Bulk-op evidence: 3 `memory.copy`, 0 `memory.fill`, no libcall — the
// segment joins are `copy_nonoverlapping` between disjoint halves of a fresh
// buffer, so they never hit the overlap path `heap_vec_shift` pins.
// Arena: 64 KiB.

extern crate alloc;

use alloc::collections::VecDeque;
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
    (h ^ v).wrapping_mul(16777619).rotate_left(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 40) as usize;
    let mut x = input2 | 1;
    let mut h: u32 = 0x811c_9dc5;

    // Start small so the first pushes fit, then wrap the ring by pushing at
    // the front, and finally grow while wrapped.
    let mut d: VecDeque<u32> = VecDeque::with_capacity(4);
    d.push_back(x);
    d.push_back(x ^ 0x0f0f_0f0f);
    d.push_front(x.rotate_left(17));

    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        if (x >> 13) & 1 == 0 {
            d.push_front(x ^ (i as u32));
        } else {
            d.push_back(x.wrapping_add(i as u32));
        }
        // Drain from the other end every few steps so the head keeps moving.
        if i % 5 == 4 {
            if let Some(front) = d.pop_front() {
                h = mix(h, front);
            }
        }
        if i % 7 == 6 {
            if let Some(back) = d.pop_back() {
                h = mix(h, back);
            }
        }
    }

    // Logical indexing across the wrap point.
    let len = d.len();
    if len > 0 {
        for k in 0..4u32 {
            // The index is built in u32 BEFORE the `as usize`: `usize` is 64
            // bits natively and 32 on wasm, so `(input2 as usize) + 9` wraps
            // on one target only (the campaign-13 `rodata_big` rule).
            let idx = (input2.wrapping_add(k.wrapping_mul(3)) % (len as u32)) as usize;
            if let Some(v) = d.get(idx) {
                h = mix(h, *v);
            }
        }
    }

    // In-order drain.
    let mut count: u32 = 0;
    while let Some(v) = d.pop_front() {
        h = mix(h, v);
        count += 1;
    }
    h.wrapping_add(count)
}

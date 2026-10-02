// W3 — `BinaryHeap<u32>`: push (sift-UP loop) and pop (sift-DOWN loop) mixed
// at runtime, then `into_sorted_vec` (which is a heapsort in place followed
// by a reverse). The sift loops are index arithmetic over a `Vec` that is
// reallocated as it grows, and `into_sorted_vec` converts the heap into a
// sorted `Vec` without allocating again.
//
// The result hashes the pop ORDER, so a sift loop that swaps the wrong pair
// changes the answer even though the multiset is unchanged.
//
// Bulk-op evidence: 1 `memory.copy` (the backing `Vec`'s realloc), 0
// `memory.fill`, no libcall. Arena: 64 KiB.

extern crate alloc;

use alloc::collections::BinaryHeap;
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
    (h ^ v).wrapping_mul(2654435761).rotate_left(5)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 40) as usize;
    let mut x = input2 | 1;
    let mut h: u32 = 0x9e37_79b9;

    let mut heap: BinaryHeap<u32> = BinaryHeap::new();
    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        // Duplicates on purpose: equal keys exercise the sift comparisons'
        // tie handling.
        heap.push(x >> 20);
        if i % 3 == 2 {
            if let Some(top) = heap.pop() {
                h = mix(h, top);
            }
        }
        if let Some(peek) = heap.peek() {
            h = mix(h, *peek);
        }
    }

    let rest = heap.len() as u32;
    let sorted = heap.into_sorted_vec();
    for i in 0..sorted.len() {
        h = mix(h, sorted[i]).rotate_left(1);
    }
    h.wrapping_add(rest).wrapping_add(sorted.len() as u32)
}

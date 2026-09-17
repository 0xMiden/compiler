// W1 — the non-shifting edit surface of `Vec`: `truncate`, `resize`,
// `swap_remove` (swap with the last element, then pop — no bulk move),
// `retain` and `dedup` (a `BackshiftOnDrop` that moves ONE element at a time
// with a shift of at least one element, so source and destination never
// overlap) and `pop`. Every index is a runtime value taken from the inputs.
//
// This is the passing sibling of `heap_vec_shift`/`heap_vec_shift_u8`: it
// bounds those findings to the OVERLAPPING bulk moves (`insert`/`remove`/
// `drain`) by showing the same container, allocator and element type agree
// with native when no copy overlaps.
//
// Bulk-op evidence: the realloc `memory.copy`s only; the per-element moves of
// `retain`/`dedup` are constant-size (4 bytes) and become plain loads/stores.
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
    (h ^ v).wrapping_mul(2654435761).rotate_left(5)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 34) as usize;
    let mut x = input2 | 1;

    let mut v: Vec<u32> = Vec::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x >> 3);
    }

    // swap_remove at a runtime index (0, mid, len-1 are all reachable).
    let mut h: u32 = 0x1357_9bdf;
    if !v.is_empty() {
        let i = (input2 as usize) % v.len();
        h = mix(h, v.swap_remove(i));
    }

    // pop, then grow back with resize (fills with a constant) and truncate.
    if let Some(last) = v.pop() {
        h = mix(h, last);
    }
    let target = (input1 >> 8) as usize % 40;
    v.resize(target, x ^ 0xa5a5_a5a5);
    v.truncate((input2 >> 5) as usize % 24);

    // retain and dedup: one-element back-shifts, never an overlapping copy.
    let keep = (input1 >> 3) & 3;
    v.retain(|e| (e & 3) != keep);
    for i in 0..v.len() {
        if i & 1 == 0 {
            v[i] = v[i] & !1;
        } else {
            v[i] = v[i] | 1;
        }
    }
    v.dedup();

    for i in 0..v.len() {
        h = mix(h, v[i]);
    }
    h.wrapping_add((v.len() as u32) << 3)
}

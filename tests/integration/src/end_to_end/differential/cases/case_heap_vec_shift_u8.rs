// W1 — the UPWARD overlapping in-buffer move on a `Vec<u8>`: `insert(i, b)`
// shifts the tail up by ONE BYTE. Source and destination are one byte apart,
// so they can never share a residue mod 4 and the copy NEVER takes the
// element fast path — it takes the byte fallback loop, which copies upward.
//
// Copying upward is correct only when the destination is BELOW the source; an
// upward loop shifting a range up re-reads bytes it has already overwritten.
// `heap_vec_remove_u8` is the same byte loop in the other direction and pins
// that it is correct there, so the two together separate direction from
// alignment (`heap_vec_shift` / `heap_vec_drain` are the 4-aligned pair).
//
// Bulk-op evidence: 2 `memory.copy` in the wasm, each a `hir.mem_cpy` over
// `ptr<u8, byte>` with a runtime count. Arena: 64 KiB.

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

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = 4 + (input1 % 30) as usize;
    let mut x = input2 | 1;

    let mut v: Vec<u8> = Vec::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push((x >> 19) as u8);
    }

    let i = (input2 as usize) % (v.len() + 1);
    v.insert(i, 0xa7);

    let mut h: u32 = 0x811c_9dc5 ^ (i as u32);
    for k in 0..v.len() {
        h = (h ^ (v[k] as u32)).wrapping_mul(16777619);
    }
    h.wrapping_add(v.len() as u32)
}

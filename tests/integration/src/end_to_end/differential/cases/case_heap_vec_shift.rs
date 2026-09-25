// W1 — the UPWARD overlapping in-buffer move of `Vec<u32>`: `insert(i, v)`
// shifts the tail up by one element, i.e. a single `ptr::copy` (memmove)
// inside one buffer whose destination is ABOVE its source and whose ranges
// overlap by all but one element. One half of the 2x2 direction/alignment
// matrix this module pins; the other three are `heap_vec_drain` (u32, down),
// `heap_vec_shift_u8` (bytes, up) and `heap_vec_remove_u8` (bytes, down).
//
// For a 4-byte element type the three operands of that copy are all
// 4-aligned (`src % 4 == dst % 4 == count % 4 == 0`), so codegen takes the
// element fast path `::miden::core::mem::memcopy_elements`, whose overlap
// assertion is direction-independent.
//
// Bulk-op evidence: 2 `memory.copy` in the wasm (realloc + the shift), each a
// `hir.mem_cpy` over `ptr<u8, byte>` with a runtime count. Arena: 64 KiB.

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
    (h ^ v).wrapping_mul(16777619).rotate_left(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = 4 + (input1 % 30) as usize;
    let mut x = input2 | 1;

    let mut v: Vec<u32> = Vec::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x >> 2);
    }

    // insert at 0 / mid / len-1 / len depending on the input.
    let i = (input2 as usize) % (v.len() + 1);
    v.insert(i, 0xfeed_face);

    let mut h: u32 = mix(0x811c_9dc5, i as u32);
    for k in 0..v.len() {
        h = mix(h, v[k]);
    }
    h.wrapping_add(v.len() as u32)
}

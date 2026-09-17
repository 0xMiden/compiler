// W1 — the DOWNWARD overlapping in-buffer moves of `Vec<u32>`: `remove(j)`
// shifts the tail down by one element and `drain(a..b)` shifts it down by
// `b - a` elements when the `Drain` guard drops. Both are one `ptr::copy`
// (memmove) inside a single buffer with the destination BELOW the source.
//
// As in `heap_vec_shift` all three operands are 4-aligned, so codegen takes
// `::miden::core::mem::memcopy_elements`. A downward overlapping copy is what
// a forward copy loop would get RIGHT, which is exactly what makes this the
// interesting half: the core-lib assertion rejects overlap in both
// directions, so the direction the hardware could serve is refused too.
//
// Bulk-op evidence: 3 `memory.copy` in the wasm (realloc, remove, and the
// drain's tail move), each a `hir.mem_cpy` over `ptr<u8, byte>` with a
// runtime count. Arena: 64 KiB.

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
    let n = 8 + (input1 % 26) as usize;
    let mut x = input2 | 1;

    let mut v: Vec<u32> = Vec::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x >> 2);
    }

    // remove at a runtime index: tail shifts down by one element.
    let j = (input2 as usize) % v.len();
    let mut h: u32 = mix(0x811c_9dc5, v.remove(j));

    // drain a runtime sub-range: tail shifts down by the drained length.
    let a = ((input2 >> 7) as usize) % (v.len() + 1);
    let b = a + (((input1 >> 5) as usize) % (v.len() + 1 - a));
    for d in v.drain(a..b) {
        h = mix(h, d);
    }

    for k in 0..v.len() {
        h = mix(h, v[k]);
    }
    h.wrapping_add(v.len() as u32)
}

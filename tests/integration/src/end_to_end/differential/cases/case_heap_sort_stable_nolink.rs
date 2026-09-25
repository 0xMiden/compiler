// W3 — the minimal producer of `core`'s STABLE sort on a heap buffer:
// `Vec<u32>::sort()` at a runtime length. The guest builds and links fine;
// the ASSEMBLER rejects the program, because the size-optimized stable sort
// `core::slice::sort::stable::tiny::mergesort` calls itself.
//
// This is the same failure mode as `corelib::core_select_nth_nolink`
// (`median_of_medians`), not a `memcmp` link failure: the whole `cargo test`
// process survives it, so this case can be batched with the rest of the
// module.
//
// `heap_sort` is the working replacement (a hand-written iterative merge
// sort plus `sort_unstable*`), and pins that the ALLOCATING half of a stable
// sort is not what is unsupported — the recursion is.

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
    let n = (input1 % 41) as usize;
    let mut v: Vec<u32> = Vec::new();
    let mut x = input2 | 1;
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x >> 20);
    }
    v.sort();
    let mut h: u32 = 0;
    for i in 0..v.len() {
        h = (h ^ v[i]).wrapping_mul(16777619);
    }
    h.wrapping_add(v.len() as u32)
}

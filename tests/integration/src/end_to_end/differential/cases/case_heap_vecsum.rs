// Campaign-34 bring-up: the smallest `alloc` program the corpus can run.
// A case-local bump allocator (identical code on both targets) backs a
// `Vec<u32>` grown by `input1 & 63` pushes; the result is a rolling hash of
// the elements plus the length, so nothing address-dependent escapes.
//
// The allocator is reset at entry: the native `cdylib` stays loaded across
// every input pair of a run while the VM starts fresh, so without the reset
// the arena would leak natively only and a late pair would OOM on one side.
//
// Bulk-op evidence (wasm built exactly as the harness builds it): the realloc
// path is `memory.copy` — 2 `memory.copy`, 0 `memory.fill`, no `memcpy`/
// `memmove` libcall — and `midenc` represents each as one `hir.mem_cpy`.
// Arena: 64 KiB of `.bss`, 16-byte aligned (a `[u8; N]` static is only
// byte-aligned, and the u32 loads carry `align=4` memargs).

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
    let n = (input1 & 63) as usize;
    let mut v: Vec<u32> = Vec::new();
    let mut x = input2;
    for _ in 0..n {
        x = x.wrapping_mul(2654435761).wrapping_add(1);
        v.push(x);
    }
    let mut h: u32 = 0x9e37_79b9;
    for i in 0..v.len() {
        h = h.rotate_left(5) ^ v[i];
    }
    h.wrapping_add(v.len() as u32)
}

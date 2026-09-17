// W1 — element sizes through the realloc copy: `Vec<u8>` (size 1, the
// unaligned tail), `Vec<u64>` (size 8, two felts per element, 8-byte
// alignment), `Vec<[u8; 3]>` (odd element size — the buffer length is never
// a multiple of 4 for most counts) and `Vec<(u8, u32)>` (size 8 with a
// 3-byte padding hole, so the copy moves uninitialised padding too).
//
// Only growth-by-push and reads: no `insert`/`remove`/`drain`, so every bulk
// copy is between two DISJOINT buffers and the element fast path is legal.
// Result: one rolling hash over all four vectors plus their lengths.
//
// Bulk-op evidence: 2 `memory.copy` in the wasm (LLVM shares the realloc
// copy between the same-layout element types), no `memcpy`/`memmove` libcall
// and no `memory.fill`. Arena: 64 KiB.

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
    let n = (input1 % 34) as usize;
    let mut x = input2 | 1;

    let mut b: Vec<u8> = Vec::new();
    let mut q: Vec<u64> = Vec::new();
    let mut t: Vec<[u8; 3]> = Vec::new();
    let mut p: Vec<(u8, u32)> = Vec::new();

    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        b.push(x as u8);
        q.push(((x as u64) << 32) | (i as u64 ^ (x >> 16) as u64));
        t.push([x as u8, (x >> 8) as u8, (x >> 16) as u8]);
        p.push((((x >> 24) as u8), x.rotate_left(11)));
    }

    let mut h: u32 = 0x9e37_79b9;
    for i in 0..b.len() {
        h = (h ^ (b[i] as u32)).wrapping_mul(16777619);
    }
    for i in 0..q.len() {
        let v = q[i];
        h = h.rotate_left(7) ^ (v as u32) ^ ((v >> 32) as u32);
    }
    for i in 0..t.len() {
        let e = t[i];
        h = h.rotate_left(3) ^ (e[0] as u32) ^ ((e[1] as u32) << 8) ^ ((e[2] as u32) << 16);
    }
    for i in 0..p.len() {
        let (a, c) = p[i];
        h = h.rotate_left(11) ^ (a as u32).wrapping_add(c);
    }
    h.wrapping_add(b.len() as u32)
        .wrapping_add((q.len() as u32) << 8)
        .wrapping_add((t.len() as u32) << 16)
        .wrapping_add((p.len() as u32) << 24)
}

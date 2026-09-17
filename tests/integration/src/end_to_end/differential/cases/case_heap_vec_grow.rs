// W1 — Vec growth: every realloc boundary a `Vec<u32>` crosses.
//
// `Vec::new()` starts at capacity 0 and doubles 4 -> 8 -> 16 -> 32 -> 64, so
// `input1 % 34` pushes cross four capacity boundaries; each crossing is a
// `RawVec::grow_amortized` -> `finish_grow` -> `GlobalAlloc::realloc`, which
// the default `GlobalAlloc::realloc` implements as alloc + `copy_nonoverlapping`
// + dealloc. `extend_from_slice` and `reserve` reach the same path with an
// exact (non-amortized) request. Result: a rolling hash of the elements plus
// the lengths, so a buffer copied short or to the wrong place shows.
//
// Bulk-op evidence: 3 `memory.copy`, 0 `memory.fill`, no libcall; each is one
// `hir.mem_cpy %dst, %src, %count : (ptr<u8, byte>, ptr<u8, byte>, u32)`, i.e.
// byte-typed pointers and a RUNTIME count, so codegen takes the runtime
// 4-alignment test between `memcopy_elements` and the byte fallback loop.
// Arena: 64 KiB of `.bss`.

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
    let n = (input1 % 34) as usize;
    let mut x = input2 | 1;

    // Amortized growth: one push at a time across 4/8/16/32/64.
    let mut v: Vec<u32> = Vec::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x);
    }

    // Exact growth: `extend_from_slice` of a runtime-length window, twice, so
    // the second one reallocs a non-empty buffer.
    let mut w: Vec<u32> = Vec::new();
    let k = n & 7;
    let block = [x, x ^ 0x5555_5555, x.rotate_left(13), x.wrapping_add(7), !x, x >> 3, x << 5, 9];
    w.extend_from_slice(&block[..k]);
    w.extend_from_slice(&block[..k]);

    // Reserve past the current capacity, then fill it.
    let mut r: Vec<u32> = Vec::with_capacity(1);
    r.push(x);
    r.reserve(n);
    for i in 0..n {
        r.push(x.wrapping_add(i as u32));
    }

    let mut h: u32 = 0x811c_9dc5;
    for i in 0..v.len() {
        h = mix(h, v[i]);
    }
    for i in 0..w.len() {
        h = mix(h, w[i]);
    }
    for i in 0..r.len() {
        h = mix(h, r[i]);
    }
    h.wrapping_add((v.len() as u32) << 16)
        .wrapping_add((w.len() as u32) << 8)
        .wrapping_add(r.len() as u32)
}

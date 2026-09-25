// W2 — `Box` shapes: a `Box<[u32; 8]>` (a whole array moved into the heap
// and written through), a `Vec<Box<u32>>` (a vector of pointers, so the
// realloc copy moves POINTERS — 4 bytes on wasm, 8 natively, which is why
// only the pointed-to values and the length may escape), `Option<Box<u32>>`
// exercising the null-pointer niche in both states, and `Box<u64>` (8-byte
// alignment out of the bump allocator).
//
// Every `Box` is dropped by the bump allocator's no-op `dealloc`, so the
// arena only ever grows; the reset at entry is what keeps that sound.
//
// Bulk-op evidence: 1 `memory.copy` (the `Vec<Box<u32>>` realloc), no
// `memory.fill`, no libcall. Arena: 64 KiB.

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
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
    (h ^ v).wrapping_mul(2654435761).rotate_left(9)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 24) as usize;
    let mut x = input2 | 1;

    // A boxed array, written through the Box.
    let mut arr: Box<[u32; 8]> = Box::new([0; 8]);
    for i in 0..8 {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        arr[i] = x ^ (i as u32);
    }
    arr.swap((input1 & 7) as usize, (input2 & 7) as usize);

    // A vector of boxed values: the realloc copy moves pointers.
    let mut v: Vec<Box<u32>> = Vec::new();
    for i in 0..n {
        v.push(Box::new(x.wrapping_add(i as u32).rotate_left(3)));
    }

    // The null-pointer niche of `Option<Box<T>>`, in both states.
    let maybe: Option<Box<u32>> = if input1 & 1 == 0 { Some(Box::new(x >> 7)) } else { None };
    let niche = match maybe {
        Some(b) => mix(1, *b),
        None => 0xdead_0000,
    };

    // An 8-byte-aligned allocation out of the bump allocator.
    let wide: Box<u64> = Box::new(((x as u64) << 32) | (input1 as u64));

    let mut h: u32 = 0x9e37_79b9;
    for i in 0..8 {
        h = mix(h, arr[i]);
    }
    for i in 0..v.len() {
        h = mix(h, *v[i]);
    }
    h = mix(h, niche);
    h = mix(h, (*wide >> 32) as u32 ^ (*wide as u32));
    h.wrapping_add(v.len() as u32)
}

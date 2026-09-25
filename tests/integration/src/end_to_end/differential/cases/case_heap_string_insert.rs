// W4 — `String::insert(idx, ch)`: the third container that shifts a buffer
// UP with an overlapping `ptr::copy`, after `Vec::insert` (`heap_vec_shift`,
// `heap_vec_shift_u8`) and `BTreeMap` (`heap_btree`). A `String` is a byte
// buffer, so inserting a one-byte `char` moves the tail up by exactly one
// byte — source and destination can never share a residue mod 4, so the copy
// takes the memcpy BYTE fallback loop, which copies upward and re-reads bytes
// it has already overwritten.
//
// Only ASCII is pushed, so every index is a char boundary and the insert
// position is a plain runtime value; the string is hashed byte by byte, and
// no `str` comparison appears anywhere (a runtime-length `memcmp` does not
// link on this target).
//
// Bulk-op evidence: 2 `memory.copy`, 0 `memory.fill`, no libcall.
// Arena: 64 KiB.

extern crate alloc;

use alloc::string::String;
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

    let mut s = String::new();
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        s.push((b'a' + ((x >> 19) % 26) as u8) as char);
    }

    let at = (input2 % ((s.len() as u32) + 1)) as usize;
    s.insert(at, '#');

    let mut h: u32 = 0x811c_9dc5 ^ (at as u32);
    for b in s.bytes() {
        h = (h ^ (b as u32)).wrapping_mul(16777619);
    }
    h.wrapping_add(s.len() as u32)
}

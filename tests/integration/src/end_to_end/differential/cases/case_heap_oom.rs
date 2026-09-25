// W5a — allocator exhaustion as a TRAP-PARITY case (`run_case_traps`): the
// arena is deliberately tiny (1 KiB) and the odd rows (`input1 & 1 == 1`)
// push far more than fits, so `GlobalAlloc::alloc` returns null,
// `RawVec` calls `handle_alloc_error`, and the default (stable) allocation
// error handler panics. On wasm that panic is a `unreachable` (guests build
// with `-Cpanic=immediate-abort`) and on the host the trapping case header's
// panic handler `_exit(101)`s the forked child, so both sides must agree per
// input on trap-or-value.
//
// The even rows stay well inside the arena and are value-checked in the same
// run, so the case pins BOTH halves of the oracle: OOM traps on both targets,
// and a fitting allocation returns the same number on both.
//
// No `alloc_error_handler` is needed: since the default handler was
// stabilized, an `alloc`-using `no_std` crate with a `#[global_allocator]`
// gets a panicking one for free.
//
// Arena: 1 KiB (small on purpose), reset at entry as everywhere else.

extern crate alloc;

use alloc::vec::Vec;
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
};

const ARENA_SIZE: usize = 1 << 10;

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
    // Odd rows ask for far more than the 1 KiB arena (the doubling `Vec`
    // requests old+new, so 4000 u32s never fit); even rows ask for at most
    // 32 u32s and always fit.
    let n = if input1 & 1 == 1 { 4000 } else { (input1 >> 8) % 33 } as usize;
    let mut v: Vec<u32> = Vec::new();
    let mut x = input2 | 1;
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        v.push(x >> 4);
    }
    let mut h: u32 = 0x811c_9dc5;
    for i in 0..v.len() {
        h = (h ^ v[i]).wrapping_mul(16777619);
    }
    h.wrapping_add(v.len() as u32)
}

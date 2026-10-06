// W2 — `Box<dyn Trait>`: heap-allocated trait objects dispatched through
// their vtables. A `Vec<Box<dyn Op>>` holds three implementors chosen by a
// runtime index, and the whole vector is read through
// `core::hint::black_box` so LLVM cannot devirtualize the dispatch: with
// nightly-2026-09-01 a constant table of implementors is folded into a
// switch of direct calls, which would measure nothing (the same rule the
// `calls::indirect_spill_bb` fn-pointer case follows).
//
// Vtable dispatch is a wasm `call_indirect` through the function table, so
// this is the one case in the module that crosses the indirect-call path
// with heap-owned receivers.
//
// Bulk-op evidence: 1 `memory.copy` (the vector of fat pointers), no
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

trait Op {
    fn apply(&self, v: u32) -> u32;
    fn tag(&self) -> u32;
}

struct AddOp(u32);
struct XorOp(u32);
struct RotOp(u32);

impl Op for AddOp {
    fn apply(&self, v: u32) -> u32 {
        v.wrapping_add(self.0)
    }

    fn tag(&self) -> u32 {
        1
    }
}

impl Op for XorOp {
    fn apply(&self, v: u32) -> u32 {
        v ^ self.0
    }

    fn tag(&self) -> u32 {
        2
    }
}

impl Op for RotOp {
    fn apply(&self, v: u32) -> u32 {
        v.rotate_left(self.0 & 31)
    }

    fn tag(&self) -> u32 {
        3
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 17) as usize;
    let mut x = input2 | 1;

    let mut ops: Vec<Box<dyn Op>> = Vec::new();
    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        let op: Box<dyn Op> = match (x.wrapping_add(i as u32)) % 3 {
            0 => Box::new(AddOp(x >> 3)),
            1 => Box::new(XorOp(x >> 7)),
            _ => Box::new(RotOp(x >> 11)),
        };
        ops.push(op);
    }

    let table = core::hint::black_box(&ops);
    let mut acc = input2;
    let mut tags: u32 = 0;
    for i in 0..table.len() {
        let op = &table[i];
        acc = op.apply(acc);
        tags = tags.rotate_left(2) ^ op.tag();
    }
    acc.wrapping_add(tags).wrapping_add(table.len() as u32)
}

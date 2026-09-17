// W2 — a `Box`-linked list: `Option<Box<Node>>` links (the null-pointer
// niche again, now recursive), built by pushing at the front, traversed by
// following the links, reversed in place by re-linking, and DROPPED
// ITERATIVELY with an `Option::take` loop.
//
// The iterative drop is mandatory: `Node`'s automatic drop glue calls itself
// through `next`, and a recursive call graph is rejected by the assembler
// ("found a cycle in the call graph"). Taking every link out before the node
// dies leaves each `Node` with `next == None` when it is dropped, and the
// list length is bounded anyway (<= 33 nodes).
//
// Bulk-op evidence: 0 `memory.copy`, 0 `memory.fill`, no libcall — a linked
// list only ever moves one node at a time, so this case isolates the
// allocator and the pointer traffic from the bulk-copy findings.
// Arena: 64 KiB.

extern crate alloc;

use alloc::boxed::Box;
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

struct Node {
    value: u32,
    next: Option<Box<Node>>,
}

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(2654435761).rotate_left(11)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 34) as usize;
    let mut x = input2 | 1;

    // Build by pushing at the front.
    let mut head: Option<Box<Node>> = None;
    for _ in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        head = Some(Box::new(Node { value: x >> 5, next: head }));
    }

    // Traverse.
    let mut h: u32 = 0x1357_9bdf;
    let mut walk = head.as_deref();
    let mut len: u32 = 0;
    while let Some(node) = walk {
        h = mix(h, node.value);
        len += 1;
        walk = node.next.as_deref();
    }

    // Reverse in place by re-linking, then traverse again: the hash order
    // flips, so a link followed in the wrong direction shows.
    let mut reversed: Option<Box<Node>> = None;
    let mut cursor = head.take();
    while let Some(mut node) = cursor {
        cursor = node.next.take();
        node.next = reversed;
        reversed = Some(node);
    }
    let mut walk = reversed.as_deref();
    while let Some(node) = walk {
        h = mix(h, node.value ^ 0x5555_5555);
        walk = node.next.as_deref();
    }

    // Iterative drop: every node is unlinked before it dies.
    let mut cursor = reversed.take();
    while let Some(mut node) = cursor {
        cursor = node.next.take();
        h = mix(h, node.value & 0xff);
    }

    h.wrapping_add(len)
}

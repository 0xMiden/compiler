// W3 — `BTreeMap<u32, u32>` and `BTreeSet<u32>` across node splits. A B-tree
// leaf holds 11 keys, so more than 11 inserts force a split and more than
// ~130 force a height-2 tree; this case inserts up to 160 keys. Splitting
// moves key and value slices between nodes with `ptr::copy_nonoverlapping`
// (disjoint nodes) and shifts keys inside one node with `ptr::copy`
// (OVERLAPPING, but at element granularity inside a node buffer).
//
// `remove` drives the other half: leaf underflow steals from a sibling or
// merges two nodes, both of which move slices again.
//
// Results: a rolling hash over an in-ORDER iteration (so a mis-split shows as
// a wrong order, not just a wrong multiset), the values fetched by `get` at
// runtime keys, and the lengths.
//
// Bulk-op evidence: 84 `memory.copy`, 0 `memory.fill`, no libcall — by far
// the densest bulk-copy shape in the corpus (every node split, steal and
// merge is its own copy site). Arena: 256 KiB (a B-tree node is ~130 bytes
// and the bump allocator never frees).

extern crate alloc;

use alloc::collections::{BTreeMap, BTreeSet};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
};

const ARENA_SIZE: usize = 1 << 18;

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
    let n = (input1 % 161) as usize;
    let mut x = input2 | 1;
    let mut h: u32 = 0x811c_9dc5;

    let mut map: BTreeMap<u32, u32> = BTreeMap::new();
    let mut set: BTreeSet<u32> = BTreeSet::new();
    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        // Keys collide on purpose (mod 512): re-inserting an existing key
        // replaces the value rather than splitting.
        let key = (x >> 11) % 512;
        if let Some(old) = map.insert(key, x ^ (i as u32)) {
            h = mix(h, old);
        }
        set.insert(key >> 2);
    }

    // Lookups at runtime keys, present and absent.
    for k in 0..6 {
        let key = ((input2 >> (k * 3)) ^ (k as u32)) % 512;
        match map.get(&key) {
            Some(v) => h = mix(h, *v),
            None => h = mix(h, 0xabad_1dea),
        }
        h = mix(h, map.contains_key(&key) as u32);
    }

    // Removals: leaf underflow, sibling steal, node merge.
    for k in 0..12 {
        let key = ((input1 >> (k & 15)).wrapping_add(k as u32)) % 512;
        if let Some(v) = map.remove(&key) {
            h = mix(h, v);
        }
        set.remove(&(key >> 2));
    }

    // In-order iteration: order is part of the answer.
    for (k, v) in map.iter() {
        h = mix(h, *k).rotate_left(1) ^ *v;
    }
    // A range query over the set.
    let lo = (input2 >> 4) % 128;
    let hi = lo + ((input1 >> 6) % 64);
    for v in set.range(lo..=hi) {
        h = mix(h, *v);
    }

    h.wrapping_add(map.len() as u32).wrapping_add((set.len() as u32) << 12)
}

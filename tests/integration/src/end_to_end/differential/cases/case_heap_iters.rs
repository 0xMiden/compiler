// W3 — `collect::<Vec<_>>()` out of iterator pipelines. `collect` from a
// sized iterator preallocates exactly (`TrustedLen` / `size_hint`), while
// `filter` makes the length unknown so the vector grows by reallocation
// instead; `chain` and `zip` compose two size hints, and `rev` walks a
// double-ended iterator backwards. `extend` appends into an existing buffer.
//
// The consumers (`sum`, `max`, `min`, `position`, `fold`) then read the
// collected buffers back, so a mis-sized allocation or a short copy shows in
// the hash rather than in a pointer.
//
// Bulk-op evidence: 1 `memory.copy`, 0 `memory.fill`, no libcall.
// Arena: 64 KiB.

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
    let seed = input2 | 1;
    let mut h: u32 = 0x9e37_79b9;

    // Exactly-sized collect out of a map.
    let base: Vec<u32> = (0..n as u32)
        .map(|i| seed.wrapping_mul(i.wrapping_add(3)).rotate_left(i & 31))
        .collect();

    // Unknown-length collect out of a filter: the vector grows.
    let keep = (input1 >> 7) & 3;
    let odd: Vec<u32> = base.iter().copied().filter(|v| (v & 3) != keep).collect();

    // chain + zip + rev.
    let chained: Vec<u32> =
        base.iter().copied().chain(odd.iter().copied()).map(|v| v >> 1).collect();
    let zipped: Vec<u32> = base
        .iter()
        .copied()
        .zip(odd.iter().copied().rev())
        .map(|(a, b)| a.wrapping_sub(b))
        .collect();

    // extend into an existing buffer.
    let mut acc: Vec<u32> = Vec::with_capacity(2);
    acc.push(seed);
    acc.extend(zipped.iter().copied().take(9));
    acc.extend(chained.iter().copied().skip(3).step_by(2));

    for v in base.iter() {
        h = mix(h, *v);
    }
    for v in odd.iter() {
        h = mix(h, *v).rotate_left(1);
    }
    for v in chained.iter() {
        h = mix(h, *v).rotate_left(2);
    }
    for v in zipped.iter() {
        h = mix(h, *v).rotate_left(3);
    }
    for v in acc.iter() {
        h = mix(h, *v).rotate_left(4);
    }

    let total: u32 = base.iter().fold(0u32, |a, v| a.wrapping_add(*v));
    let biggest = chained.iter().copied().max().unwrap_or(0);
    let smallest = zipped.iter().copied().min().unwrap_or(0xffff_ffff);
    let where_big = chained.iter().position(|v| *v == biggest).unwrap_or(99) as u32;

    h = mix(h, total);
    h = mix(h, biggest);
    h = mix(h, smallest);
    h = mix(h, where_big);
    h.wrapping_add(base.len() as u32)
        .wrapping_add((odd.len() as u32) << 6)
        .wrapping_add((chained.len() as u32) << 12)
        .wrapping_add((zipped.len() as u32) << 18)
        .wrapping_add((acc.len() as u32) << 24)
}

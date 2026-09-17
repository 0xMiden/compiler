// W4 — `String` as a growing byte buffer: `push` (one byte for ASCII, up to
// three for the non-ASCII chars below, through `encode_utf8`), `push_str`,
// `truncate`, `pop` (which has to find the previous boundary) and
// `String::from_utf8` of a `Vec<u8>` built at runtime. The reads are the
// UTF-8 walkers: `chars`, `chars().rev()`, `char_indices`, `bytes`,
// `is_char_boundary`. `String::insert` is deliberately absent — it is an
// overlapping byte move and has its own case, `heap_string_insert`.
//
// Non-ASCII on purpose: the emitted characters cycle through 1-, 2- and
// 3-byte encodings, so the buffer length and the char count differ and a
// mis-sized realloc copy shows in both.
//
// No `==` on `str`/`String` anywhere: a runtime-length string compare is a
// `memcmp` libcall that does not link on this target (the `core` link-reach
// map, `corelib::core_eq_reach`). Everything is compared element-wise.
//
// Results: a byte hash of the string plus its byte length and char count.
//
// Bulk-op evidence: 2 `memory.copy`, 0 `memory.fill`, no libcall.
// Arena: 64 KiB.

extern crate alloc;

use alloc::{string::String, vec::Vec};
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

// 1-, 2- and 3-byte UTF-8 encodings, so byte length != char count.
const ALPHABET: [char; 8] = ['a', 'Z', '0', '~', 'é', 'ß', '€', '№'];

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(16777619).rotate_left(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 34) as usize;
    let mut x = input2 | 1;

    let mut s = String::new();
    for i in 0..n {
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        s.push(ALPHABET[((x >> 17) & 7) as usize]);
        if i % 5 == 4 {
            s.push_str("--|");
        }
    }

    // `String::insert` lives in its own case (`heap_string_insert`): it is a
    // byte-granular UPWARD overlapping copy and diverges. Appending does not.
    let at = (input2 % ((s.len() as u32) + 1)) as usize;
    if at == s.len() {
        s.push('#');
    }
    let mut cut = (input1 >> 9) as usize % (s.len() + 1);
    while cut < s.len() && !s.is_char_boundary(cut) {
        cut += 1;
    }
    s.truncate(cut);
    if let Some(last) = s.pop() {
        x ^= last as u32;
    }

    // A String built from bytes, validated at runtime.
    let mut raw: Vec<u8> = Vec::new();
    for i in 0..n {
        raw.push(b'A' + ((x.wrapping_add(i as u32) >> 3) % 26) as u8);
    }
    let built = match String::from_utf8(raw) {
        Ok(t) => t,
        Err(_) => String::new(),
    };

    // Walk both strings in every direction.
    let mut h: u32 = 0x811c_9dc5 ^ x;
    let mut chars: u32 = 0;
    for b in s.bytes() {
        h = mix(h, b as u32);
    }
    for c in s.chars() {
        h = mix(h, c as u32).rotate_left(1);
        chars += 1;
    }
    for c in s.chars().rev() {
        h = mix(h, (c as u32) ^ 0x5555_5555);
    }
    for (idx, c) in s.char_indices() {
        h = mix(h, (idx as u32).wrapping_mul(31) ^ (c as u32));
    }
    for b in built.bytes() {
        h = mix(h, b as u32).rotate_left(2);
    }

    h.wrapping_add(s.len() as u32)
        .wrapping_add(chars << 8)
        .wrapping_add((built.len() as u32) << 16)
}

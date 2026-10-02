// W4 — `core::fmt` driving a heap buffer: `format!` (which allocates and
// grows a `String` through `fmt::Write`), `to_string`, `write!` into a
// `String` via `core::fmt::Write`, the integer `Display` paths for u32/i32/
// u64 (including `i32::MIN`), hex/octal/binary/width/zero-pad formatting,
// `{:?}` on a derived `Debug`, and `str::parse::<u32>()` back out of the
// formatted text.
//
// The parse is done on a RUNTIME-length slice of the formatted string: with a
// constant length LLVM folds `str::parse` at compile time and the runtime
// parser is never reached (the `core` link-reach map).
//
// No `==` on `str`: the round trip is checked by comparing the PARSED NUMBER
// with the original, not the text.
//
// Results: a byte hash of every formatted string plus their lengths.
//
// Bulk-op evidence: 7 `memory.copy`, 0 `memory.fill`, no libcall.
// Arena: 64 KiB.

extern crate alloc;

use alloc::{
    format,
    string::{String, ToString},
};
use core::{
    alloc::{GlobalAlloc, Layout},
    cell::UnsafeCell,
    fmt::Write,
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

#[derive(Debug)]
struct Record {
    tag: u8,
    span: u32,
    flag: bool,
}

fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(2654435761).rotate_left(5)
}

fn hash_str(h: u32, s: &str) -> u32 {
    let mut acc = h;
    for b in s.bytes() {
        acc = mix(acc, b as u32);
    }
    acc.wrapping_add(s.len() as u32)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    unsafe { NEXT = 0 };
    let n = (input1 % 12) as usize;
    let mut h: u32 = 0x9e37_79b9;

    // Display of the four integer shapes, including i32::MIN.
    let decimal = format!("{}", input1);
    let signed = format!("{}", input2 as i32);
    let wide = format!("{}", ((input1 as u64) << 32) | (input2 as u64));
    h = hash_str(h, &decimal);
    h = hash_str(h, &signed);
    h = hash_str(h, &wide);
    h = hash_str(h, &input1.to_string());
    h = hash_str(h, &(input2 as i32).to_string());

    // Radix, width, sign and padding.
    let radix = format!("{:x}|{:X}|{:o}|{:b}", input1, input2, input1 >> 3, input2 & 0xff);
    let padded = format!("[{:>10}][{:<8}][{:08x}][{:+}]", input1 % 1000, input2 % 97, input1, 7i32);
    h = hash_str(h, &radix);
    h = hash_str(h, &padded);

    // Derived Debug over a small record.
    let rec = Record {
        tag: (input1 >> 24) as u8,
        span: input2 ^ 0x0f0f_0f0f,
        flag: input1 & 1 == 0,
    };
    h = hash_str(h, &format!("{:?}", rec));

    // write! into a String through core::fmt::Write, in a loop.
    let mut buf = String::new();
    for i in 0..n {
        let _ = write!(buf, "{}:{:x};", i, input2.wrapping_mul(i as u32 + 1));
    }
    h = hash_str(h, &buf);

    // Parse back out of a runtime-length slice of the formatted text.
    let end = 1 + ((input2 as usize) % decimal.len());
    let head = &decimal[..end];
    let parsed = match head.parse::<u32>() {
        Ok(v) => v,
        Err(_) => 0xdead_beef,
    };
    h = mix(h, parsed);
    let round = match decimal.parse::<u32>() {
        Ok(v) => (v == input1) as u32,
        Err(_) => 2,
    };
    h = mix(h, round);

    h.wrapping_add(buf.len() as u32)
}

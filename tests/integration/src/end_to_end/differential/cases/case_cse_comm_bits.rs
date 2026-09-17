// Commutative `arith.band` / `arith.bor` / `arith.bxor` over the same two SSA
// values in both operand orders, same hatch as `cse_comm_arith`: volatile byte
// reads issued in the opposite order for the second copy of each op.
//
// `&`, `|`, `^` on `u32` translate to `arith.band` / `bor` / `bxor` (the
// frontend's `builder.band`/`bor`/`bxor`), all three of which carry the
// `Commutative` trait. `arith.and` / `or` / `xor` (the i1 logical ops) have no
// wasm producer at all, so they are not reachable from plain Rust.

use core::ptr::read_volatile;

#[inline(never)]
fn band_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let x1 = a1 & b1;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let x2 = b2 & a2;
    x1.wrapping_sub(x2.rotate_left(3))
}

#[inline(never)]
fn bor_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let o1 = a1 | b1;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let o2 = b2 | a2;
    o1.wrapping_sub(o2.rotate_left(5))
}

#[inline(never)]
fn bxor_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let f1 = a1 ^ b1;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let f2 = b2 ^ a2;
    f1.wrapping_sub(f2.rotate_left(7))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    band_swap(p)
        ^ bor_swap(p).rotate_left(1)
        ^ bxor_swap(p).rotate_left(2)
        ^ band_swap(unsafe { p.add(2) }).rotate_left(3)
        ^ bor_swap(unsafe { p.add(2) }).rotate_left(4)
        ^ bxor_swap(unsafe { p.add(2) }).rotate_left(5)
}

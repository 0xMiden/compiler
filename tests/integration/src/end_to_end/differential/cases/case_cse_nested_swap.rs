// W3: a commutative op whose operands are themselves commutative results in
// swapped order. `(a + b) * (c ^ d)` and `(d ^ c) * (b + a)`: the outer
// `arith.mul` can only match once the inner `arith.add` and `arith.bxor` have
// merged, and CSE visits a block front to back, so the swapped copy is placed
// AFTER the original.
//
// Same volatile-byte hatch as `cse_comm_arith`; the four bytes are read in the
// order a, b, c, d for the original and d, c, b, a for the copy, so every one
// of the three ops reaches HIR with its operands swapped.

use core::ptr::read_volatile;

#[inline(never)]
fn nested_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let c1 = unsafe { read_volatile(p.add(2)) } as u32;
    let d1 = unsafe { read_volatile(p.add(3)) } as u32;
    let m1 = a1.wrapping_add(b1).wrapping_mul(c1 ^ d1);
    let d2 = unsafe { read_volatile(p.add(3)) } as u32;
    let c2 = unsafe { read_volatile(p.add(2)) } as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let m2 = (d2 ^ c2).wrapping_mul(b2.wrapping_add(a2));
    m1.rotate_left(1).wrapping_sub(m2.rotate_left(3))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 8] = [
        input1 as u8,
        input2 as u8,
        (input1 >> 8) as u8,
        (input2 >> 8) as u8,
        (input1 >> 16) as u8,
        (input2 >> 16) as u8,
        (input1 >> 24) as u8,
        (input2 >> 24) as u8,
    ];
    let p = cell.as_ptr();
    nested_swap(p)
        ^ nested_swap(unsafe { p.add(1) }).rotate_left(1)
        ^ nested_swap(unsafe { p.add(2) }).rotate_left(2)
        ^ nested_swap(unsafe { p.add(4) }).rotate_left(3)
}

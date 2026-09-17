// W3: the same swapped-operand shape as `cse_comm_arith`, but with an OPAQUE
// WRITE between the two candidates. The reloads must not merge across the
// write, so the two commutative ops must not merge either -- and here the
// merge would be a real miscompile, because the write changes the bytes
// between the two reads, so the two sums are genuinely different values.
//
// Three write kinds, each of which CSE must treat as a barrier: an
// `#[inline(never)]` helper (a `hir.exec`, which implements no
// `MemoryEffectOpInterface` at all, so CSE assumes a write), a plain store
// through a `*mut u8`, and a `write_volatile`.

use core::ptr::{read_volatile, write_volatile};

#[inline(never)]
fn poke(p: *mut u8, v: u8) {
    unsafe { *p = v };
}

#[inline(never)]
fn call_barrier(p: *mut u8, v: u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let s1 = a1.wrapping_add(b1);
    poke(p, v);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let s2 = b2.wrapping_add(a2);
    s1.rotate_left(1).wrapping_sub(s2.rotate_left(3))
}

#[inline(never)]
fn store_barrier(p: *mut u8, v: u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let s1 = a1 & b1;
    unsafe { write_volatile(p.add(1), v) };
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let s2 = b2 & a2;
    s1.rotate_left(1).wrapping_sub(s2.rotate_left(3))
}

// The barrier lands on a byte NEITHER read touches: the merge is still
// forbidden (CSE has no alias analysis), and the two sums are equal, so this
// half only checks that nothing else breaks.
#[inline(never)]
fn far_barrier(p: *mut u8, v: u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let s1 = a1 ^ b1;
    unsafe { write_volatile(p.add(3), v) };
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let s2 = b2 ^ a2;
    s1.rotate_left(1).wrapping_sub(s2.rotate_left(3))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut cell: [u8; 4] =
        [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_mut_ptr();
    let v = (input1 >> 8) as u8;
    let r1 = call_barrier(p, v);
    cell = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_mut_ptr();
    let r2 = store_barrier(p, v);
    cell = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_mut_ptr();
    let r3 = far_barrier(p, v);
    r1 ^ r2.rotate_left(1) ^ r3.rotate_left(2)
}

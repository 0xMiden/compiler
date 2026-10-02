// W2 oracle: NON-commutative ops with swapped operands must never merge.
//
// Exactly the shape of `cse_comm_arith` (volatile byte reads, second copy
// issued in the opposite order, both results live), but over `arith.sub`,
// `arith.shl`, `arith.shr` and `arith.ashr` -- none of which carries the
// `Commutative` trait. A merge here is a MISCOMPILE, and it is visible in the
// value: `a - b` and `b - a` differ for every `a != b`, and so do the shifts.

use core::ptr::read_volatile;

#[inline(never)]
fn sub_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let d1 = a1.wrapping_sub(b1);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let d2 = b2.wrapping_sub(a2);
    d1.rotate_left(1).wrapping_sub(d2.rotate_left(3))
}

#[inline(never)]
fn shl_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let d1 = a1.wrapping_shl(b1);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let d2 = b2.wrapping_shl(a2);
    d1.rotate_left(1).wrapping_sub(d2.rotate_left(3))
}

#[inline(never)]
fn shr_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let d1 = a1.wrapping_shr(b1);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let d2 = b2.wrapping_shr(a2);
    d1.rotate_left(1).wrapping_sub(d2.rotate_left(3))
}

// Signed shift-right: `arith.ashr`.
#[inline(never)]
fn ashr_swap(p: *const u8) -> u32 {
    let a1 = (unsafe { read_volatile(p) } as i8) as i32;
    let b1 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let d1 = a1.wrapping_shr(b1 as u32);
    let b2 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let a2 = (unsafe { read_volatile(p) } as i8) as i32;
    let d2 = b2.wrapping_shr(a2 as u32);
    (d1 as u32).rotate_left(1).wrapping_sub((d2 as u32).rotate_left(3))
}

// `wrapping_sub` vs `unchecked_sub` on the SAME operand pair in the same
// order: the LLVM flags differ (`sub` vs `sub nuw nsw`), but wasm has one
// `i32.sub` and the frontend always builds `arith.sub` with the `wrapping`
// overflow property, so a merge here is correct. Swapping the operands of the
// unchecked form would be UB in the source program, so that half of the
// question is unreachable, not just unproduced. The larger byte is taken
// first so the unchecked subtraction never underflows.
#[inline(never)]
fn sub_flags(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let hi1 = if a1 > b1 { a1 } else { b1 };
    let lo1 = if a1 > b1 { b1 } else { a1 };
    let d1 = hi1.wrapping_sub(lo1);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let hi2 = if a2 > b2 { a2 } else { b2 };
    let lo2 = if a2 > b2 { b2 } else { a2 };
    let d2 = unsafe { hi2.unchecked_sub(lo2) };
    d1.rotate_left(1).wrapping_sub(d2.rotate_left(3))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    sub_swap(p)
        ^ shl_swap(p).rotate_left(1)
        ^ shr_swap(p).rotate_left(2)
        ^ ashr_swap(p).rotate_left(3)
        ^ sub_flags(p).rotate_left(4)
        ^ sub_swap(unsafe { p.add(2) }).rotate_left(5)
        ^ shr_swap(unsafe { p.add(2) }).rotate_left(6)
}

// W2 oracle for the ordering comparisons: `arith.lt` / `lte` / `gt` / `gte`
// with swapped operands must never merge. None of the four carries the
// `Commutative` trait (only `Eq` and `Neq` do), and `a < b` is not `b < a`, so
// a merge is visible in the value on every unequal pair.
//
// Both signednesses: the unsigned comparisons go through
// `pop2_bitcasted(U32)`, the signed ones through the plain `pop2`, but both
// land on the same four `arith` ops.
//
// The two flags are combined with a bare `wrapping_sub` for the reason given
// in `cse_comm_cmp`: a `bool << k` makes LLVM build a `select` of two
// constants, which parks the probe bytes in wasm locals, and the resulting
// `hir.store_local` (a Write) would block the reload merge -- leaving the
// non-merge below unproven rather than proven.

use core::ptr::read_volatile;

#[inline(never)]
fn ult_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let l1 = (a1 < b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let l2 = (b2 < a2) as u32;
    l1.wrapping_sub(l2)
}

#[inline(never)]
fn ule_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let l1 = (a1 <= b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let l2 = (b2 <= a2) as u32;
    l1.wrapping_sub(l2)
}

#[inline(never)]
fn ugt_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let l1 = (a1 > b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let l2 = (b2 > a2) as u32;
    l1.wrapping_sub(l2)
}

#[inline(never)]
fn uge_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let l1 = (a1 >= b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let l2 = (b2 >= a2) as u32;
    l1.wrapping_sub(l2)
}

#[inline(never)]
fn slt_swap(p: *const u8) -> u32 {
    let a1 = (unsafe { read_volatile(p) } as i8) as i32;
    let b1 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let l1 = (a1 < b1) as u32;
    let b2 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let a2 = (unsafe { read_volatile(p) } as i8) as i32;
    let l2 = (b2 < a2) as u32;
    l1.wrapping_sub(l2)
}

#[inline(never)]
fn sge_swap(p: *const u8) -> u32 {
    let a1 = (unsafe { read_volatile(p) } as i8) as i32;
    let b1 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let l1 = (a1 >= b1) as u32;
    let b2 = (unsafe { read_volatile(p.add(1)) } as i8) as i32;
    let a2 = (unsafe { read_volatile(p) } as i8) as i32;
    let l2 = (b2 >= a2) as u32;
    l1.wrapping_sub(l2)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    ult_swap(p)
        ^ ule_swap(p).rotate_left(1)
        ^ ugt_swap(p).rotate_left(2)
        ^ uge_swap(p).rotate_left(3)
        ^ slt_swap(p).rotate_left(4)
        ^ sge_swap(p).rotate_left(5)
        ^ ult_swap(unsafe { p.add(2) }).rotate_left(6)
        ^ sge_swap(unsafe { p.add(2) }).rotate_left(7)
}

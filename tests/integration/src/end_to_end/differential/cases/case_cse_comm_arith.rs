// Commutative `arith.add` / `arith.mul` over the SAME two SSA values in BOTH
// operand orders, under dominance, in one block.
//
// The escape hatch is volatile BYTE reads. LLVM's EarlyCSE/GVN merge `x + y`
// with `y + x` whenever both are visible on the same SSA values, so a
// hand-written pair of orders never survives to the wasm. Two volatile reads
// of one address are distinct LLVM values, so LLVM keeps both ops; the wasm
// stack order follows the (unreorderable) volatile load order, which is how
// the second op reaches HIR with its operands swapped. In HIR the volatility
// is gone and a 1-byte access carries no alignment `assertz` (the Write that
// blocks a heap-load merge for every wider access), so CSE merges the reloads
// first and only then can the commutative multiset key match.

use core::ptr::read_volatile;

// Widen a probe byte into a full 32-bit operand. Pure, so the two copies merge
// under CSE once the loads behind them have.
fn w(x: u8) -> u32 {
    (x as u32).wrapping_mul(0x0101_0101)
}

#[inline(never)]
fn add_swap(p: *const u8) -> u32 {
    let a1 = w(unsafe { read_volatile(p) });
    let b1 = w(unsafe { read_volatile(p.add(1)) });
    let s1 = a1.wrapping_add(b1);
    let b2 = w(unsafe { read_volatile(p.add(1)) });
    let a2 = w(unsafe { read_volatile(p) });
    let s2 = b2.wrapping_add(a2);
    s1.wrapping_sub(s2.rotate_left(3))
}

#[inline(never)]
fn mul_swap(p: *const u8) -> u32 {
    let a1 = w(unsafe { read_volatile(p) });
    let b1 = w(unsafe { read_volatile(p.add(1)) });
    let s1 = a1.wrapping_mul(b1);
    let b2 = w(unsafe { read_volatile(p.add(1)) });
    let a2 = w(unsafe { read_volatile(p) });
    let s2 = b2.wrapping_mul(a2);
    s1.wrapping_sub(s2.rotate_left(5))
}

// The swapped copy inside a dominated block rather than a later statement:
// the merge must not happen, because CSE's load_local/load candidates are
// block-local and the two `hir.load`s therefore never become one value.
#[inline(never)]
fn add_swap_if(p: *const u8, c: bool) -> u32 {
    let a1 = w(unsafe { read_volatile(p) });
    let b1 = w(unsafe { read_volatile(p.add(1)) });
    let s1 = a1.wrapping_add(b1);
    if c {
        let b2 = w(unsafe { read_volatile(p.add(1)) });
        let a2 = w(unsafe { read_volatile(p) });
        s1.wrapping_sub(b2.wrapping_add(a2).rotate_left(3))
    } else {
        s1.rotate_left(7)
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    add_swap(p)
        ^ mul_swap(p).rotate_left(1)
        ^ add_swap_if(p, input1 & 1 != 0).rotate_left(2)
        ^ add_swap(unsafe { p.add(2) }).rotate_left(3)
}

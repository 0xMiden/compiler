// Commutative `arith.eq` / `arith.neq` over the same two SSA values in both
// operand orders, same volatile-byte hatch as `cse_comm_arith`.
//
// The two boolean results are combined with a bare `wrapping_sub`, not with
// shifts: a `bool << k` makes LLVM materialize the flag through a `select` of
// two constants, which parks the probe bytes in wasm locals -- and a
// `local.set` is a `hir.store_local`, a Write that blocks the reload merge the
// whole shape depends on. `e1 - e2` is a sufficient oracle: it is zero exactly
// when the two flags agree, so a wrong merge pins it to zero.
//
// `arith.min` / `arith.max` are `Commutative` too, but the wasm frontend never
// builds them: wasm has no i32 min/max operator, and `core::cmp::min`/`max`
// lower to a compare plus a select. They have no plain-Rust producer.

use core::ptr::read_volatile;

#[inline(never)]
fn eq_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let e1 = (a1 == b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let e2 = (b2 == a2) as u32;
    e1.wrapping_sub(e2)
}

#[inline(never)]
fn neq_swap(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let n1 = (a1 != b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let n2 = (b2 != a2) as u32;
    n1.wrapping_sub(n2)
}

// Mixed: an `==` and a `!=` over the same swapped pair must NOT merge with
// each other (different op names), while each merges with its own twin.
#[inline(never)]
fn eq_neq_mix(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let e1 = (a1 == b1) as u32;
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let n1 = (b2 != a2) as u32;
    let a3 = unsafe { read_volatile(p) } as u32;
    let b3 = unsafe { read_volatile(p.add(1)) } as u32;
    let e2 = (b3 == a3) as u32;
    e1.wrapping_sub(n1).wrapping_sub(e2)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let cell: [u8; 4] = [input1 as u8, input2 as u8, (input1 >> 16) as u8, (input2 >> 16) as u8];
    let p = cell.as_ptr();
    eq_swap(p)
        ^ neq_swap(p).rotate_left(1)
        ^ eq_neq_mix(p).rotate_left(2)
        ^ eq_swap(unsafe { p.add(2) }).rotate_left(3)
        ^ neq_swap(unsafe { p.add(2) }).rotate_left(4)
}

// W3 corner cases for the commutative multiset key, all on the volatile-byte
// hatch of `cse_comm_arith`:
//
//  - a commutative op over {a, a} beside the ops over {a, b}: the multiset
//    [h(a), h(a)] must not match [h(a), h(b)]. The op has to be `*`, not `+`:
//    LLVM rewrites `a + a` to `a << 1`, so it never reaches HIR as an
//    `arith.add` with two equal operands. The POSITIVE half -- two `a * a`
//    merging with each other -- has no producer: duplicating one value on the
//    wasm stack needs a `local.tee`, and reading it twice from two volatile
//    loads makes LLVM park the bytes in locals, and either way the resulting
//    `hir.store_local` is a Write that blocks the reload merge the shape
//    depends on.
//  - `a * b` vs `c * a` where `c` is a DIFFERENT SSA value that holds the same
//    byte at runtime (a second cell whose contents the entrypoint keeps equal).
//    The key is SSA identity (`DefaultValueHasher` / `DefaultValueEquivalence`),
//    so they must not merge however equal the runtime values are.
//  - three `*` over the pair {a, b} in mixed orders plus a fourth over {a, a}:
//    the three collapse to one, the fourth stays.

use core::ptr::read_volatile;

// `p` and `q` are distinct addresses that the entrypoint fills with the same
// byte, so `b` and `c` are equal at runtime and distinct in SSA.
#[inline(never)]
fn equal_but_distinct(p: *const u8, q: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let s1 = a1.wrapping_mul(b1);
    let c2 = unsafe { read_volatile(q) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let s2 = c2.wrapping_mul(a2);
    s1.rotate_left(1).wrapping_sub(s2.rotate_left(3))
}

#[inline(never)]
fn three_plus_one(p: *const u8) -> u32 {
    let a1 = unsafe { read_volatile(p) } as u32;
    let b1 = unsafe { read_volatile(p.add(1)) } as u32;
    let s1 = a1.wrapping_mul(b1);
    let b2 = unsafe { read_volatile(p.add(1)) } as u32;
    let a2 = unsafe { read_volatile(p) } as u32;
    let s2 = b2.wrapping_mul(a2);
    let a3 = unsafe { read_volatile(p) } as u32;
    let b3 = unsafe { read_volatile(p.add(1)) } as u32;
    let s3 = a3.wrapping_mul(b3);
    let a4 = unsafe { read_volatile(p) } as u32;
    let a5 = unsafe { read_volatile(p) } as u32;
    let s4 = a4.wrapping_mul(a5);
    s1.rotate_left(1)
        .wrapping_sub(s2.rotate_left(3))
        .wrapping_sub(s3.rotate_left(5))
        .wrapping_sub(s4.rotate_left(7))
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    // Every (probe, twin) pair holds the same byte at two distinct addresses,
    // so `equal_but_distinct` always sees operands that agree at runtime and
    // are distinct in SSA.
    let cell: [u8; 6] = [
        input1 as u8,
        input2 as u8,
        input2 as u8,
        (input1 >> 16) as u8,
        (input2 >> 16) as u8,
        (input2 >> 16) as u8,
    ];
    let p = cell.as_ptr();
    equal_but_distinct(p, unsafe { p.add(2) })
        ^ three_plus_one(p).rotate_left(1)
        ^ equal_but_distinct(unsafe { p.add(3) }, unsafe { p.add(5) }).rotate_left(2)
        ^ three_plus_one(unsafe { p.add(3) }).rotate_left(3)
}

// A static table of function pointers indexed by runtime data, lowered to a
// wasm funcref table + `call_indirect`. The table is read through
// `black_box`: since nightly-2026-09-01, LLVM devirtualizes an index into a
// constant fn-pointer table into a switch of direct calls, which would leave
// the case without a single `call_indirect`.
use core::hint::black_box;

#[inline(never)]
fn op_add(a: u32, b: u32) -> u32 {
    a.wrapping_add(b)
}

#[inline(never)]
fn op_sub(a: u32, b: u32) -> u32 {
    a.wrapping_sub(b)
}

#[inline(never)]
fn op_xor(a: u32, b: u32) -> u32 {
    (a ^ b).wrapping_add(7)
}

#[inline(never)]
fn op_mix(a: u32, b: u32) -> u32 {
    (a | b).wrapping_mul(2654435761).rotate_left(5)
}

static OPS: [fn(u32, u32) -> u32; 4] = [op_add, op_sub, op_xor, op_mix];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let f = black_box(&OPS)[(input1 & 3) as usize];
    let g = black_box(&OPS)[(input2 & 3) as usize];
    f(input1, input2).wrapping_add(g(input2, input1))
}

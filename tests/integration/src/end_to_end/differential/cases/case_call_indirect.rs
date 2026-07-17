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

// A static table of function pointers indexed by runtime data. LLVM does not
// devirtualize loads of fn pointers from a static array without PGO, so this
// reliably lowers to a wasm funcref table + `call_indirect`.
static OPS: [fn(u32, u32) -> u32; 4] = [op_add, op_sub, op_xor, op_mix];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let f = OPS[(input1 & 3) as usize];
    let g = OPS[(input2 & 3) as usize];
    f(input1, input2).wrapping_add(g(input2, input1))
}

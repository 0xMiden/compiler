// A user function deliberately named `__indirect_function_table_0` — the exact
// symbol the frontend generates for the lowered funcref table of table 0. Every
// module symbol is a producer-controlled string, so the table-lowering probes
// the symbol table and bumps a counter until the generated name is free; this
// case forces that collision-rename path while still dispatching indirectly.

#[unsafe(no_mangle)]
pub extern "C" fn __indirect_function_table_0(x: u32) -> u32 {
    x.wrapping_mul(0x0101_0101).rotate_left(7)
}

#[inline(never)]
fn op_gray(a: u32, b: u32) -> u32 {
    (a ^ (a >> 1)).wrapping_add(b)
}

#[inline(never)]
fn op_lerp(a: u32, b: u32) -> u32 {
    a.wrapping_add(b.wrapping_sub(a) >> 3)
}

// Runtime-indexed fn-pointer load: survives as `call_indirect`, which lazily
// lowers the funcref table and hits the reserved-name collision.
static OPS: [fn(u32, u32) -> u32; 2] = [op_gray, op_lerp];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let f = OPS[(input1 & 1) as usize];
    let mixed = f(input1, input2);
    mixed.wrapping_add(__indirect_function_table_0(input2))
}

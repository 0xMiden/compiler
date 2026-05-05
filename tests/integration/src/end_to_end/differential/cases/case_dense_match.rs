// Dense integer match intended to lower through wasm `br_table`.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    match input1 & 7 {
        0 => arm0(input1, input2),
        1 => arm1(input1, input2),
        2 => arm2(input1, input2),
        3 => arm3(input1, input2),
        4 => arm4(input1, input2),
        5 => arm5(input1, input2),
        6 => arm6(input1, input2),
        _ => arm7(input1, input2),
    }
}

#[inline(never)]
fn arm0(a: u32, b: u32) -> u32 {
    a.wrapping_add(b ^ 0x1357_2468)
}

#[inline(never)]
fn arm1(a: u32, b: u32) -> u32 {
    b.wrapping_sub(a ^ 0x2468_1357)
}

#[inline(never)]
fn arm2(a: u32, b: u32) -> u32 {
    a.wrapping_add(17).wrapping_sub(b & 0x00ff_00ff)
}

#[inline(never)]
fn arm3(a: u32, b: u32) -> u32 {
    (a | 0x0f0f_0f0f).wrapping_add(b ^ 31)
}

#[inline(never)]
fn arm4(a: u32, b: u32) -> u32 {
    (b | 0xf0f0_f0f0).wrapping_sub(a ^ 47)
}

#[inline(never)]
fn arm5(a: u32, b: u32) -> u32 {
    (a & b).wrapping_add(0x55aa_aa55)
}

#[inline(never)]
fn arm6(a: u32, b: u32) -> u32 {
    (a ^ b).wrapping_sub(0xaa55_55aa)
}

#[inline(never)]
fn arm7(a: u32, b: u32) -> u32 {
    a.wrapping_sub(b).wrapping_add(0x1020_3040)
}

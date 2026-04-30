// Sparse match with a default-heavy selector range.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let key = input1 & 31;
    match key {
        0 => branch0(input1, input2),
        3 => branch3(input1, input2),
        8 => branch8(input1, input2),
        13 => branch13(input1, input2),
        21 => branch21(input1, input2),
        31 => branch31(input1, input2),
        _ => fallback(input1, input2, key),
    }
}

#[inline(never)]
fn branch0(a: u32, b: u32) -> u32 {
    a.wrapping_add(b).wrapping_add(0x0000_0101)
}

#[inline(never)]
fn branch3(a: u32, b: u32) -> u32 {
    a.wrapping_sub(b ^ 0x0000_0303)
}

#[inline(never)]
fn branch8(a: u32, b: u32) -> u32 {
    (a & 0x0fff_ffff).wrapping_add(b ^ 0x0000_0808)
}

#[inline(never)]
fn branch13(a: u32, b: u32) -> u32 {
    (b | 0x0101_0101).wrapping_sub(a ^ 0x0000_1313)
}

#[inline(never)]
fn branch21(a: u32, b: u32) -> u32 {
    (a ^ b).wrapping_add(0x0000_2121)
}

#[inline(never)]
fn branch31(a: u32, b: u32) -> u32 {
    a.wrapping_add(0x3131_0000).wrapping_sub(b)
}

#[inline(never)]
fn fallback(a: u32, b: u32, key: u32) -> u32 {
    a.wrapping_add(key).wrapping_sub(b ^ 0x7777_7777)
}

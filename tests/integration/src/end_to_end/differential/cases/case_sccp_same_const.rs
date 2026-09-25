// Three arms with three DIFFERENT side effects that all yield the same
// constant `5`. The side effects force three distinct basic blocks; the yielded
// value is identical on every edge, which is the one lattice join where a merge
// may legally become a constant rather than overdefined.

use core::hint::black_box;

#[inline(never)]
fn arm_a(x: u32) -> u32 {
    x.wrapping_mul(0x9e37_79b9)
}

#[inline(never)]
fn arm_b(x: u32) -> u32 {
    x.rotate_left(11)
}

#[inline(never)]
fn arm_c(x: u32) -> u32 {
    x ^ 0x27d4_eb2f
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let k = input2 % 3;
    let v: u32 = if k == 0 {
        black_box(arm_a(input1));
        5
    } else if k == 1 {
        black_box(arm_b(input1));
        5
    } else {
        black_box(arm_c(input1));
        5
    };
    v.wrapping_mul(input1 | 1)
}

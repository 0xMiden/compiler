// A flag that is the SAME constant on every edge of a merge, then used as the
// condition of a later branch: the textbook shape SCCP's dead-code analysis is
// built for -- prove the flag constant, prove one successor edge unreachable,
// erase the arm. Both arms of the producing merge end in a different opaque
// call, so the merge itself cannot be collapsed for lack of distinct blocks.

use core::hint::black_box;

#[inline(never)]
fn arm_a(x: u32) -> u32 {
    x.wrapping_mul(0x9e37_79b9)
}

#[inline(never)]
fn arm_b(x: u32) -> u32 {
    x.rotate_left(17)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let flag: u32 = if input2 & 1 == 0 {
        black_box(arm_a(input1));
        1
    } else {
        black_box(arm_b(input1));
        1
    };

    if flag == 1 {
        input1.wrapping_mul(3).wrapping_add(input2)
    } else {
        // Unreachable for every input: `flag` is 1 on both edges.
        input1.wrapping_add(0xdead_beef).rotate_left(9)
    }
}

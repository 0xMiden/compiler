// An `if`-with-result whose else arm ends in an opaque call, so LLVM cannot
// collapse the merge into a `select` and has to keep both branches. The merged
// value is a constant on one edge and a computed value on the other -- the
// classic "constant meets overdefined" lattice join.

use core::hint::black_box;

#[inline(never)]
fn opaque(x: u32) -> u32 {
    x.wrapping_mul(3).wrapping_add(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let v = if input2 & 1 == 0 {
        black_box(opaque(input1 ^ 0x5bf0_3635));
        5u32
    } else {
        opaque(input1)
    };
    v ^ input1
}

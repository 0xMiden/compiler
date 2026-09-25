// Nested merges: an inner `if`-with-result whose BOTH arms are constants feeds
// an outer `if`-with-result whose other edge is computed. If the inner merge
// ever reached SCCP as a block argument, its lattice would be a two-constant
// join feeding a second join one level up.

use core::hint::black_box;

#[inline(never)]
fn opaque(x: u32) -> u32 {
    x.wrapping_mul(0x85eb_ca6b)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let outer = if input2 & 1 == 0 {
        let inner = if input2 & 2 == 0 {
            black_box(opaque(input1));
            13u32
        } else {
            black_box(opaque(input1 ^ 1));
            29u32
        };
        inner
    } else {
        opaque(input1 | 1)
    };
    outer.wrapping_mul(input1 | 1)
}

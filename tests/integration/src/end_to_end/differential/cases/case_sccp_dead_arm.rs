// A dense `match` over a masked selector, so the `br_table`'s DEFAULT target is
// an arm no input can select, plus a chain of values that only that arm's own
// successor consumes -- a dead block defining a value used only by other dead
// blocks. The live arms each end in an opaque call so the table survives, and
// the default arm computes something different from all of them so a wrongly
// live default edge shows up as a value mismatch rather than as dead code.

use core::hint::black_box;

#[inline(never)]
fn mix(x: u32, k: u32) -> u32 {
    x.wrapping_mul(0x9e37_79b9).rotate_left(k & 31)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let sel = black_box(input2 & 3);
    let v = match sel {
        0 => mix(input1, 1),
        1 => mix(input1, 5),
        2 => mix(input1, 9),
        3 => mix(input1, 13),
        _ => {
            // No input reaches this arm: `sel` is masked to 0..=3.
            let dead0 = input1.wrapping_add(0xdead_beef);
            let dead1 = dead0.rotate_left(7);
            let dead2 = dead1 ^ dead0;
            mix(dead2, 21)
        }
    };
    v.wrapping_add(input1)
}

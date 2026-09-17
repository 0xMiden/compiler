// Four merges of four DIFFERENT types in one function, chosen so the same
// numeric values recur across them: `5` at u32 and at u64, `1` at i32 and as
// `true` at i1. The constant folder keys uniqued constants by (dialect, value,
// type), so a type-blind key would let the u64 `5` and the u32 `5` -- or the
// i32 `1` and the i1 `true` -- share one materialized `arith.constant`, which
// is an F18-shaped wrong-width push rather than anything visible in an IR dump.

use core::hint::black_box;

#[inline(never)]
fn tick(x: u32) -> u32 {
    x.wrapping_add(0x9e37_79b9)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u32 = if input2 & 1 == 0 {
        black_box(tick(input1));
        5
    } else {
        black_box(tick(input1 ^ 1));
        9
    };
    let b: u64 = if input2 & 2 == 0 {
        black_box(tick(input1));
        5
    } else {
        black_box(tick(input1 ^ 2));
        0x0000_0007_0000_0005
    };
    let f: bool = if input2 & 4 == 0 {
        black_box(tick(input1));
        true
    } else {
        black_box(tick(input1 ^ 4));
        false
    };
    let s: i32 = if input2 & 8 == 0 {
        black_box(tick(input1));
        1
    } else {
        black_box(tick(input1 ^ 8));
        -1
    };

    let mut r = a;
    r = r.wrapping_add(b as u32);
    r = r.wrapping_add((b >> 32) as u32);
    r = r.wrapping_add(s as u32);
    if f {
        r ^= 0xa5a5_a5a5;
    }
    r
}

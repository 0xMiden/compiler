// A dense four-way `match` whose arms each yield a DIFFERENT constant and each
// end in an opaque call, so LLVM keeps the `br_table` instead of folding the
// four incoming constants into a table lookup or a select chain. This is the
// "two different constants meet" join, four-wide, plus a second merge (`acc`)
// that carries a computed value on every edge.

use core::hint::black_box;

#[inline(never)]
fn arm_a(x: u32) -> u32 {
    x.wrapping_mul(0x9e37_79b9)
}

#[inline(never)]
fn arm_b(x: u32) -> u32 {
    x.rotate_left(7)
}

#[inline(never)]
fn arm_c(x: u32) -> u32 {
    x ^ 0x5bf0_3635
}

#[inline(never)]
fn arm_d(x: u32) -> u32 {
    x.wrapping_sub(0x1234_5678)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let acc;
    let v: u32 = match input2 & 3 {
        0 => {
            acc = arm_a(input1);
            11
        }
        1 => {
            acc = arm_b(input1);
            22
        }
        2 => {
            acc = arm_c(input1);
            33
        }
        _ => {
            acc = arm_d(input1);
            44
        }
    };
    black_box(acc).wrapping_add(v)
}

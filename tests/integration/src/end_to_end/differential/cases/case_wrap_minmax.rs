// MIN/MAX wrapping boundaries. Edge relations asserted by the pinned grid:
// wrapping_neg(MIN) == MIN and wrapping_abs(MIN) == MIN (i32 and i64),
// u32/i32/i64 MAX.wrapping_add(1) == MIN-or-0, MAX.wrapping_mul(MAX) == 1,
// i32::MIN as i64 (sext) and as u64, u32::MAX widening to u64 without
// truncation, and the None arms of checked_add/checked_sub/checked_neg
// (legalized by LLVM to wrapping + compare, which is the shape under test).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32;
    let w = (((input1 as u64) << 32) | input2 as u64) as i64;

    let n1 = a.wrapping_neg(); // MIN -> MIN
    let n2 = a.wrapping_abs(); // MIN -> MIN
    let n3 = w.wrapping_neg(); // i64::MIN -> i64::MIN
    let s = a as i64; // sext: MIN -> 0xFFFF_FFFF_8000_0000
    let u = a as u64; // sext + bitcast: MIN -> 0xFFFF_FFFF_8000_0000
    let z = (input1 as u64).wrapping_add(input2 as u64); // MAX+MAX widened, no wrap

    let w1 = input1.wrapping_add(input2); // u32 MAX + 1 -> 0
    let w2 = input1.wrapping_mul(input2); // u32 MAX * MAX -> 1
    let w3 = a.wrapping_add(input2 as i32); // i32 MAX + 1 -> MIN
    let w4 = w.wrapping_add(1); // i64 MAX + 1 -> i64::MIN
    let w5 = w.wrapping_mul(w | 1); // i64 wrapping mul at the extremes

    let c1 = input1.checked_add(input2).map_or(0xaaaa_5555, |v| v ^ 0x1111_1111);
    let c2 = a.checked_neg().map_or(0x3333_cccc, |v| v as u32); // None iff MIN
    let c3 = input1.checked_sub(input2).map_or(0x0f0f_f0f0, |v| v);

    let m = (n3 as u64)
        ^ z.rotate_left(3)
        ^ (s as u64).rotate_left(7)
        ^ u.rotate_left(11)
        ^ (w4 as u64).rotate_left(15)
        ^ (w5 as u64).rotate_left(19);
    (n1 as u32)
        .wrapping_add((n2 as u32).rotate_left(5))
        .wrapping_add(w1.rotate_left(9))
        .wrapping_add(w2.rotate_left(13))
        .wrapping_add((w3 as u32).rotate_left(17))
        .wrapping_add(c1.rotate_left(21))
        .wrapping_add(c2.rotate_left(25))
        .wrapping_add(c3.rotate_left(29))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

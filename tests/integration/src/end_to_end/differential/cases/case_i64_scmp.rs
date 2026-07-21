// Exercises the signed-i64 comparison emitters `lt_i64`/`lte_i64`/`gt_i64`/
// `gte_i64` in `codegen/masm/src/emit/int64.rs` (the `Type::I64` arms of the
// `binary.rs` compare dispatchers), which exec the
// `::intrinsics::i64::{lt,lte,gt,gte}` MASM intrinsics. The 64-bit operands
// mix both inputs so the sign bit varies; distinct pairs per comparison feed
// branch conditions and selects.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | input2 as u64) as i64;
    let b = (((input2 as u64) << 21) ^ (input1 as u64).wrapping_mul(0x9E37_79B9)) as i64;
    let c = (a as u64).rotate_left(17) as i64;
    let d = b.wrapping_add(0x1234_5678) ^ a;

    let mut acc: u32 = 0;
    if a < b {
        acc = acc.wrapping_add(1);
    }
    if c <= d {
        acc = acc.wrapping_add(2);
    }
    if a > d {
        acc = acc.wrapping_add(4);
    }
    if c >= b {
        acc = acc.wrapping_add(8);
    }
    // Selects driven by signed 64-bit compares (distinct operand pairs).
    let s1 = if b < c { b } else { c };
    let s2 = if d >= a { d } else { a };

    let m = (s1 as u64) ^ (s2 as u64).rotate_right(9);
    acc ^ (m as u32) ^ ((m >> 32) as u32)
}

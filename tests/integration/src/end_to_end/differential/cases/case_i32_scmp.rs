// Exercises the signed-i32 comparison arms of `codegen/masm/src/emit/binary.rs`:
// the `Type::I32` cases of `lt`/`lte`/`gt`/`gte` dispatch to the
// `::intrinsics::i32::is_lt/is_lte/is_gt/is_gte` MASM intrinsics. Operands are
// reinterpreted random u32s so the sign bit varies freely; each comparison uses
// a distinct operand pair (so LLVM cannot merge predicates), feeding both
// branch conditions and selects.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32;
    let b = input2 as i32;
    let c = (input1.rotate_left(13) ^ 0x8000_0001) as i32;
    let d = input2.wrapping_mul(0x9E37_79B9) as i32;

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
    // Selects driven by signed compares (distinct operand pairs again).
    let s1 = if b < c { b } else { c };
    let s2 = if d >= a { d } else { a };

    acc ^ (s1 as u32).rotate_left(7) ^ (s2 as u32)
}

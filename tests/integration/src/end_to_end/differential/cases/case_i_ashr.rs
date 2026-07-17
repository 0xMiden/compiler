// Exercises arithmetic shift right: the `Type::I32`/`Type::I64` arms of the
// `shr` dispatcher in `codegen/masm/src/emit/binary.rs` -> `shr_i32` (exec
// `::intrinsics::i32::checked_shr`) and `shr_i64` (exec
// `::intrinsics::i64::checked_shr`). Uses dynamic masked counts plus
// constant counts; the constant-count shifts still route through the general
// `shr` (the `shr_imm_i32`/`shr_imm_i64` variants have no non-test callers).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 as i32;
    let b = (((input1 as u64) << 32) | input2 as u64) as i64;

    let s1 = a >> (input2 & 31); // dynamic-count i32.shr_s
    let s2 = a >> 7; // constant-count i32.shr_s
    let s3 = b >> (input1 & 63); // dynamic-count i64.shr_s
    let s4 = b >> 13; // constant-count i64.shr_s

    let m = (s3 as u64) ^ (s4 as u64).rotate_left(3);
    (s1 as u32)
        .wrapping_add((s2 as u32).rotate_left(9))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

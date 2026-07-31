// Exercises the NON-STRICT signed comparison arms (`lte`/`gte`) that branches
// and selects can never reach: LLVM canonicalizes `<=`/`>=` in branch/select
// position into strict compares with inverted arms (verified in emitted WAT),
// so `le_s`/`ge_s` only appear when the boolean is materialized as a VALUE.
// Each comparison lives in its own `#[inline(never)]` helper so InstCombine
// cannot fold the predicate into surrounding arithmetic, reaching the
// `Type::I32` arms of `lte`/`gte` in `codegen/masm/src/emit/binary.rs`
// (`::intrinsics::i32::is_lte/is_gte`) and `lte_i64`/`gte_i64` in `int64.rs`
// (`::intrinsics::i64::{lte,gte}`).
#[inline(never)]
fn le64(x: i64, y: i64) -> u32 {
    (x <= y) as u32
}

#[inline(never)]
fn ge64(x: i64, y: i64) -> u32 {
    (x >= y) as u32
}

#[inline(never)]
fn le32(x: i32, y: i32) -> u32 {
    (x <= y) as u32
}

#[inline(never)]
fn ge32(x: i32, y: i32) -> u32 {
    (x >= y) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | input2 as u64) as i64;
    let b = (((input2 as u64) << 21) ^ (input1 as u64).wrapping_mul(0x9E37_79B9)) as i64;
    let c = (a as u64).rotate_left(29) as i64;
    let p = input1 as i32;
    let q = input2.wrapping_mul(0x85EB_CA6B) as i32;
    let r = (input1.rotate_left(11) ^ 0x4000_0002) as i32;

    let t1 = le64(a, b); // i64.le_s
    let t2 = ge64(b, c); // i64.ge_s
    let t3 = le32(p, q); // i32.le_s
    let t4 = ge32(q, r); // i32.ge_s

    t1 ^ (t2 << 1) ^ (t3 << 2) ^ (t4 << 3) ^ (p as u32 & 0xF0)
}

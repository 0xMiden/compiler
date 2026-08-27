// Exercises the NON-STRICT unsigned u64 comparison arm `gte_u64`
// (`::miden::intrinsics::i64` family in `codegen/masm/src/emit/int64.rs`) that
// no other case reaches: in branch/select position LLVM canonicalizes
// `>=`/`<=` into strict compares with inverted arms, and the u128 compare
// legalization only ever materializes an inline `i64.le_u` pair (which is what
// keeps `lte_u64` warm) — `i64.ge_u` appears only when the boolean is
// materialized as a VALUE inside an `#[inline(never)]` helper, exactly like
// the signed twins in `case_scmp_bool.rs`. Each helper uses a distinct operand
// pair so InstCombine cannot CSE the mirrored predicates. The u32 helper pins
// the `i32.ge_u` value form onto the (already warm) `U32Gte` arm so the
// non-strict unsigned boundary semantics (`x >= x` on forced-equal draws) are
// asserted differentially at both widths.
#[inline(never)]
fn ge64u(x: u64, y: u64) -> u32 {
    (x >= y) as u32
}

#[inline(never)]
fn le64u(x: u64, y: u64) -> u32 {
    (x <= y) as u32
}

#[inline(never)]
fn ge32u(x: u32, y: u32) -> u32 {
    (x >= y) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = ((input2 as u64) << 17) ^ (input1 as u64).wrapping_mul(0xA5A5_5A5B);
    let c: u64 = a.rotate_left(13) ^ (input2 as u64);

    let t1 = ge64u(a, b); // i64.ge_u
    let t2 = le64u(b, c); // i64.le_u (distinct pair, bounds the warm sibling)
    let t3 = ge32u(input1, input2.wrapping_mul(0x85EB_CA77)); // i32.ge_u

    t1 ^ (t2 << 1) ^ (t3 << 2) ^ (input1 & 0xF8)
}

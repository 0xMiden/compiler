// Exercises the unsigned-u64 emitter arms in `codegen/masm/src/emit/int64.rs`:
// `lt_u64`/`lte_u64`/`gt_u64`/`gte_u64` (the `i64.lt_u`-family translators
// bitcast both operands to U64, the only way U64-typed values reach the
// comparison emitters), `rotr_u64` via a dynamic-count rotate (constant-count
// rotr is turned into rotl by LLVM), and the u64 arm of `clz`. Comparisons
// feed both branches and a select. u64 division/remainder lives in the
// separate `u64_udiv` case, which aborts in the VM (U64_DIV_EVENT).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = ((input2 as u64) << 21) ^ (input1 as u64).wrapping_mul(0x9E37_79B9);
    let c: u64 = a.rotate_left((input1 % 61) + 1); // dynamic i64.rotl
    let d: u64 = b.rotate_right(input2 & 63); // dynamic i64.rotr

    // Unsigned u64 comparisons on distinct operand pairs (so LLVM cannot merge
    // predicates), feeding branch conditions and a select.
    let mut acc: u64 = 0;
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
    let sel: u64 = if b < c { b } else { c };

    // u64 leading_zeros -> i64.clz.
    let lz = a.leading_zeros();

    let m = c ^ d ^ sel ^ acc ^ (lz as u64);
    (m as u32) ^ ((m >> 32) as u32)
}

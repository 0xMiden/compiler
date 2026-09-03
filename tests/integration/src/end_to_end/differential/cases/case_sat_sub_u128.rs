// KNOWN FAILURE (guest toolchain, not the Miden compiler): `u128::saturating_sub`
// on dynamic operands, the `i64.sub128` member of the F9 family. Rust lowers
// it to `overflowing_sub` + select, and LLVM (rustc 1.97.0-nightly c935696dd /
// LLVM 22.1.4, `+wide-arithmetic` as set by cargo-miden) emits the borrow test
// `a < b` (evaluated on the difference: `diff > a` as a two-limb
// `gt_u`/`eq`/`select` chain) with the `local.get` of the difference's HIGH
// limb placed BEFORE the `i64.sub128` that defines it:
//     local.get 4              ;; hi(diff) read here: zero-initialised local
//     ... a_lo a_hi b_lo b_hi
//     i64.sub128
//     local.set 4              ;; hi(diff) is only written now
//     local.tee 5              ;; lo(diff)
//     local.get 2  i64.gt_u    ;; lo(diff) > lo(a)
//     local.get 4  local.get 3  i64.gt_u ... i64.eq  select
// so the high-limb compare uses a stale zero and the result saturates to 0
// (or fails to) wrongly. The VM executes that wasm faithfully (wasmtime 48
// with `-W wide-arithmetic=y` agrees with the MASM result); the native build
// is right. i128 `saturating_sub` and u128 `checked_sub` / `overflowing_sub`
// (add128_checked) are stackified in a valid order and pass.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b1 = ((input2 as u64) << 32) | input2 as u64;
    let a = ((a1 as u128) << 64) | b1 as u128;
    let b = ((b1 as u128) << 64) | a1 as u128;
    let v = a.saturating_sub(b);
    let m = (v as u64) ^ ((v >> 64) as u64).rotate_left(9);
    (m as u32) ^ ((m >> 32) as u32)
}

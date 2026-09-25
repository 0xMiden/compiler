// KNOWN FAILURE (guest toolchain, not the Miden compiler): `u128::saturating_add`
// on dynamic operands, the `i64.add128` member of the F9 family. Rust lowers
// it to `overflowing_add` + select, and LLVM (rustc 1.97.0-nightly c935696dd /
// LLVM 22.1.4, `+wide-arithmetic` as set by cargo-miden) emits the carry test
// `sum < a` (two-limb `lt_u`/`eq`/`select` chain) with the `local.get` of the
// sum's HIGH limb placed BEFORE the `i64.add128` that defines it:
//     local.get 4              ;; hi(sum) read here: zero-initialised local
//     ... a_lo a_hi b_lo b_hi
//     i64.add128
//     local.set 4              ;; hi(sum) is only written now
//     local.tee 5              ;; lo(sum)
//     local.get 2  i64.lt_u    ;; lo(sum) < lo(a)
//     local.get 4  local.get 3  i64.lt_u ... i64.eq  select
// so the high-limb compare uses a stale zero and the sum saturates (or fails
// to saturate) wrongly. The VM executes that wasm faithfully (wasmtime 48 with
// `-W wide-arithmetic=y` agrees with the MASM result); the native build is
// right. The same idiom on i128 (`saturating_add` / `saturating_sub`, whose
// overflow test is a sign-xor on the high limb) and u128 `checked_add` /
// `overflowing_add` (add128_checked) are stackified in a valid order and pass.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b1 = ((input2 as u64) << 32) | input2 as u64;
    let a = ((a1 as u128) << 64) | b1 as u128;
    let b = ((b1 as u128) << 64) | a1 as u128;
    let v = a.saturating_add(b);
    let m = (v as u64) ^ ((v >> 64) as u64).rotate_left(9);
    (m as u32) ^ ((m >> 32) as u32)
}

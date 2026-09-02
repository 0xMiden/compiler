// KNOWN FAILURE (guest toolchain, not the Miden compiler), simplest form:
// a non-inlined `i64::checked_mul`. LLVM (rustc 1.97.0-nightly c935696dd /
// LLVM 22.1.4, `+wide-arithmetic` as set by cargo-miden) compiles the
// helper's overflow test `hi == (lo >> 63)` as
//     local.get 2            ;; still holds the parameter y
//     local.get 1  local.get 2  i64.mul_wide_s
//     local.set 2            ;; hi overwrites local 2 only now
//     local.tee 1  i64.const 63  i64.shr_s
//     i64.eq  br_if 0
// i.e. the `local.get` of the hi result is emitted BEFORE the multiply that
// defines it, so the wasm compares y with lo >> 63 instead of hi. The VM
// executes that wasm faithfully (wasmtime agrees with the MASM result); the
// native build is right. Inlined `if let Some(x) = a.checked_mul(b)` forms,
// u64 / i32 / u128 / i128 checked multiplies and every dynamic
// `i64.mul_wide_s` product used as a VALUE (wide_mul_edges) pass.
#[inline(never)]
fn mulchk(x: i64, y: i64) -> Option<i64> {
    x.checked_mul(y)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | (input1 as u64 >> 3)) as i64;
    let b = (((input2 as u64) << 32) | input2 as u64) as i64;
    let m = match mulchk(a, b) {
        Some(x) => x as u64,
        None => 0x8888_8888_8888_8888,
    };
    (m as u32) ^ ((m >> 32) as u32)
}

// KNOWN FAILURE (guest toolchain, not the Miden compiler): the fixed-point
// multiply idiom `((a as u128 * b as u128) >> 32) as u64` in straight-line
// code — the `i64.mul_wide_u` member of the F9 family. The 32-bit shift
// straddles the limb, so the result is `(hi << 32) | (lo >> 32)`, and LLVM
// (rustc 1.97.0-nightly c935696dd / LLVM 22.1.4, `+wide-arithmetic` as set
// by cargo-miden) emits the `local.get` of the HIGH word as the first operand
// of that recombination BEFORE the `i64.mul_wide_u` that defines it (here
// the recombination is folded into the u32 result: `hi ^ (lo >> 32)`):
//     local.get 2              ;; hi read here: the zero-initialised local
//     ... a ... b
//     i64.mul_wide_u
//     local.set 2              ;; hi is only written now
//     i64.const 32  i64.shr_u  ;; lo >> 32
//     i64.xor                  ;; (stale hi) ^ (lo >> 32)
// so the wasm folds a stale value into the high half. The VM executes that
// wasm faithfully (wasmtime 48 with `-W wide-arithmetic=y` agrees with the
// MASM result); the native build is right. The same expression inside an
// `#[inline(never)]` helper or a loop, and the `>> 64` (high word only) or
// dynamic-count (`__lshrti3` libcall) forms are stackified in a valid order
// and pass (mul_hi_only, width_trees).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b = ((input2 as u64) << 32) | input2 as u64;
    let m = (((a as u128) * (b as u128)) >> 32) as u64;
    (m as u32) ^ ((m >> 32) as u32)
}

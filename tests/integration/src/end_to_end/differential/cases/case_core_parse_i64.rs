// MINIMAL REPRODUCER of a native-vs-MASM divergence (campaign 27):
// `str::parse::<i64>` of a RUNTIME-length digit slice returns the wrong
// value on the VM. At `(input1, input2) = (0, 0)` the parsed slice is `"9"`;
// native and `wasmtime` (on the harness-built wasm, `-W wide-arithmetic=y`)
// both return 9, MASM returns 0 — so the guest wasm is CORRECT and the
// miscompile is on the Miden side. The same shape with `u64`
// (`core_parse_u64`) and with `i32` agrees, and the direct signed widening
// multiply (`core_mulwide_s`) agrees, so what is wrong is specific to the
// signed 64-bit `from_str` accumulation, whose wasm carries one
// `i64.mul_wide_s` and the `hi != lo >> 63` overflow check.
// A CONSTANT-length slice is not a reproducer: LLVM folds the parse away.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let s = "9007199254740993";
    let end = 1 + (input2 as usize % 16);
    let w = &s[..end];
    match w.parse::<i64>() {
        Ok(v) => (v as u32) ^ ((v >> 32) as u32),
        Err(_) => 0xdead_beef,
    }
    .wrapping_add(input1 & 1)
}

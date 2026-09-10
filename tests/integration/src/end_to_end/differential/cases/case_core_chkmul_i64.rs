// A GUEST-TOOLCHAIN (F9) reproducer, kept because it is the smallest one in
// the corpus (campaign 27): an `i64` accumulator multiplied by the CONSTANT
// 10 with `checked_mul` inside a loop — the shape `core`'s signed `from_str`
// fast path has. LLVM compiles the overflow check to `i64.mul_wide_s`
// followed by `hi != lo >> 63`, and the resulting WASM is wrong: at
// `(0, 1)` the accumulator is 9, `9 * 10` does not overflow, and native
// returns 90 while both `wasmtime` (on the harness-built wasm, `-W
// wide-arithmetic=y`) and MASM return the overflow marker 0xdead. Since
// wasmtime agrees with MASM, the bug is in the guest toolchain's
// `+wide-arithmetic` lowering, not in `midenc` — the F9 family, with a new
// and much smaller producer than the u128 shapes.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let digits = [9u8, 0, 0, 7, 1, 9, 9, 2, 5, 4, 7, 4, 0, 9, 9, 3];
    let n = 1 + (input2 as usize % 16);
    let mut acc: i64 = 0;
    let mut i = 0usize;
    while i < n {
        acc = match acc.checked_mul(10) {
            Some(v) => v,
            None => return 0xdead,
        };
        acc = match acc.checked_add(digits[i] as i64) {
            Some(v) => v,
            None => return 0xbeef,
        };
        i += 1;
    }
    ((acc as u32) ^ ((acc >> 32) as u32)).wrapping_add(input1 & 1)
}

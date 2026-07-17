// Signed 64-bit remainder with a dynamic divisor. KNOWN FAILURE: this does
// not compile — wasm `i64.rem_s` is translated to `arith.Mod` on I64, whose
// lowering dispatches into `checked_mod`, and that match has no I64 arm
// ("not implemented: checked_mod for i64 is not supported" at
// codegen/masm/src/emit/binary.rs:665). Unlike I32RemS there is no dedicated
// wasm.I64RemS op, and no ::intrinsics::i64 mod procedure exists to back one,
// so any i64 `%` with a dynamic divisor in guest code is a hard compile-time
// panic.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = (((input1 as u64) << 32) | input2 as u64) as i64;
    // Divisor in [1, 1000] — no zero, no MIN/-1; the panic happens at compile
    // time regardless.
    let d = ((input2 % 1000) as i64) + 1;
    let r = n % d;
    (r as u32) ^ ((r >> 32) as u32) ^ (n as u32)
}

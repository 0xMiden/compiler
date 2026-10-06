// KNOWN FAILURE (guest toolchain, not the Miden compiler) at guest opt-level
// 1 only: `u128::checked_add` accumulated in a loop. LLVM (rustc
// 1.97.0-nightly c935696dd / LLVM 22.1.4, `+wide-arithmetic` as set by
// cargo-miden) at `-C opt-level=1` emits the carry test `sum < a` with the
// `local.get` of the sum's HIGH limb placed BEFORE the `i64.add128` that
// defines it (the loop body starts `local.get 1 local.get 1 ... i64.add128
// local.set 1 local.tee 9 ... i64.lt_u local.get 1 ... i64.lt_u`), so the
// wasm compares the PREVIOUS iteration's high limb (zero on the first trip)
// and takes the wrong `Some`/`None` arm. Not masked by `-C debuginfo=2`;
// at opt-level 2/3/s/z the same source is stackified in a valid order (the
// add128_checked case runs this shape at the default level and passes). The
// VM executes the wasm faithfully (wasmtime 48 agrees with the MASM result);
// the native build is right.
#[inline(never)]
fn accumulate(x0: u128, y: u128, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let (nacc, v): (u128, u64) = match acc.checked_add(y | 1) {
            Some(v) => (v, (v as u64) ^ ((v >> 64) as u64).rotate_left(9)),
            None => (acc ^ y, 0x8888_8888_8888_8888),
        };
        r ^= v.rotate_left(i);
        acc = nacc;
        i = i.wrapping_add(1);
    }
    r ^ (acc as u64)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b1 = ((input2 as u64) << 32) | input2 as u64;
    let a = ((a1 as u128) << 64) | b1 as u128;
    let b = ((b1 as u128) << 64) | a1 as u128;
    let m = accumulate(a, b, (input2 & 7).wrapping_add(1));
    (m as u32) ^ ((m >> 32) as u32)
}

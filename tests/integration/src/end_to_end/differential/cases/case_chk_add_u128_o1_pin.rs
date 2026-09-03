// Deterministic twin of case_chk_add_u128_o1 (see that file): the harness
// has no entry point that pins BOTH extra midenc flags and explicit inputs,
// so this variant bakes the (1, 1) row's operands in through
// `core::hint::black_box` and ignores its inputs; every input pair then
// exercises exactly the pinned computation (native 3584 vs the wasm's 1536
// at guest opt-level 1).
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
pub extern "C" fn entrypoint(_input1: u32, _input2: u32) -> u32 {
    // The (1, 1) row of chk_add_u128_o1, opaque to LLVM.
    let a = core::hint::black_box((1u128 << 96) | (1u128 << 64) | 1);
    let b = core::hint::black_box((1u128 << 96) | (1u128 << 32));
    let m = accumulate(a, b, core::hint::black_box(2));
    (m as u32) ^ ((m >> 32) as u32)
}

// The simplest F9 GUEST-TOOLCHAIN shape: BOTH words of one wide result used
// as values (`hi ^ lo.rotate_left(k)`), for each of the four
// `+wide-arithmetic` ops (`i64.mul_wide_u`, `i64.mul_wide_s`, `i64.add128`,
// `i64.sub128`), each in its own `#[inline(never)]` helper. A standalone
// `rustc --target wasm32-wasip1 -C target-feature=+wide-arithmetic` build of
// every helper emits the `local.get` of the HIGH word as the xor's first
// operand BEFORE the op that defines it (the helper starts with `local.get N`
// and the op's `local.set N` follows), reads the zero-initialised local, and
// disagrees with native on nearly every input at opt-level 1/2/3/s/z with
// `-C debuginfo=0` or `1`. It PASSES here only because the harness builds
// guests with `debug = 2`: the variable-location records pin the ops'
// definitions and block the sink (the sext_shapes mechanism). The test runs
// as a guard of the guest wasm actually compiled, not as proof of
// correctness; if the harness ever drops `debug = 2` it fails.
#[inline(never)]
fn mw_u(x: u64, y: u64) -> u64 {
    let p = (x as u128).wrapping_mul(y as u128);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    hi ^ lo.rotate_left(7)
}

#[inline(never)]
fn mw_s(x: i64, y: i64) -> u64 {
    let p = (x as i128).wrapping_mul(y as i128);
    let hi = (p >> 64) as i64;
    let lo = p as i64;
    (hi as u64) ^ (lo as u64).rotate_left(7)
}

#[inline(never)]
fn add_w(x: u128, y: u128) -> u64 {
    let p = x.wrapping_add(y);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    hi ^ lo.rotate_left(7)
}

#[inline(never)]
fn sub_w(x: u128, y: u128) -> u64 {
    let p = x.wrapping_sub(y);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    hi ^ lo.rotate_left(7)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b = ((input2 as u64) << 32) | input2 as u64;
    let x = ((a as u128) << 64) | b as u128;
    let y = ((b as u128) << 64) | a as u128;
    let m = mw_u(a, b)
        ^ mw_s(a as i64, b as i64).rotate_left(3)
        ^ add_w(x, y).rotate_left(5)
        ^ sub_w(x, y).rotate_left(9);
    (m as u32) ^ ((m >> 32) as u32)
}

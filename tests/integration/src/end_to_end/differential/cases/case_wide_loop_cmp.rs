// F9 GUEST-TOOLCHAIN shape masked by DWARF: a loop whose wide result's HIGH
// word is compared against a shift of its LOW word (`hi != lo >> 60`) to
// decide a `break`, for `i64.mul_wide_u`, `i64.add128` and `i64.sub128`
// (one `#[inline(never)]` helper each; the signed `mul_wide_s` form of this
// compare is the ignored checked_mul_i64 / pow_i64 family). Standalone
// `rustc --target wasm32-wasip1 -C target-feature=+wide-arithmetic` builds
// place the `local.get` of the high word BEFORE the op inside the loop body
// (the compare's first operand is read from the previous iteration's local,
// zero on the first trip): the mul form fails at every opt-level, the
// add/sub forms at opt-level 2 and 3, all with `-C debuginfo=0` or `1`.
// Passes here only because the harness builds guests with `debug = 2` (as
// wide_words); a guard of the guest wasm actually compiled.
#[inline(never)]
fn mul_loop(x0: u64, y: u64, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let p = (acc as u128).wrapping_mul(y as u128);
        let hi = (p >> 64) as u64;
        let lo = p as u64;
        if hi != (lo >> 60) {
            r ^= 0xdead_0000_0000;
            break;
        }
        r ^= lo;
        acc = lo ^ (i as u64);
        i = i.wrapping_add(1);
    }
    r ^ acc
}

#[inline(never)]
fn add_loop(x0: u128, y: u128, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let p = acc.wrapping_add(y);
        let hi = (p >> 64) as u64;
        let lo = p as u64;
        if hi != (lo >> 60) {
            r ^= 0xdead_0000_0000;
            break;
        }
        r ^= lo;
        acc = p ^ (i as u128);
        i = i.wrapping_add(1);
    }
    r ^ (acc as u64)
}

#[inline(never)]
fn sub_loop(x0: u128, y: u128, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let p = acc.wrapping_sub(y);
        let hi = (p >> 64) as u64;
        let lo = p as u64;
        if hi != (lo >> 60) {
            r ^= 0xdead_0000_0000;
            break;
        }
        r ^= lo;
        acc = p ^ (i as u128);
        i = i.wrapping_add(1);
    }
    r ^ (acc as u64)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b = ((input2 as u64) << 32) | input2 as u64;
    let x = ((a as u128) << 64) | b as u128;
    let y = ((b as u128) << 64) | a as u128;
    let n = (input2 & 7).wrapping_add(1);
    let m = mul_loop(a, b, n) ^ add_loop(x, y, n).rotate_left(5) ^ sub_loop(x, y, n).rotate_left(9);
    (m as u32) ^ ((m >> 32) as u32)
}

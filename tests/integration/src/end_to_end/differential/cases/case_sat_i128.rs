// Nearest PASSING neighbour of the sat_add_u128 / sat_sub_u128 guest-LLVM
// miscompiles: `i128::saturating_add` / `saturating_sub` on both-sign
// operands in straight-line, `#[inline(never)]` helper and loop forms. The
// signed forms test overflow with a sign xor on the high limb of the
// `i64.add128` / `i64.sub128` result (no `sum < a` limb compare), and LLVM
// stackifies them in a valid order (standalone builds agree with native at
// every opt-level and debuginfo level). Operands are limb-swapped so grid
// rows with both input high bits set overflow past i128::MIN/MAX.
#[inline(never)]
fn sat(x: i128, y: i128) -> u64 {
    let s = x.saturating_add(y);
    let d = x.saturating_sub(y);
    (s as u64)
        ^ ((s >> 64) as u64).rotate_left(9)
        ^ ((d as u64) ^ ((d >> 64) as u64).rotate_left(5)).rotate_left(21)
}

#[inline(never)]
fn sat_loop(x0: i128, y: i128, n: u32) -> u64 {
    let mut r: u64 = 0;
    let mut acc = x0;
    let mut i = 0u32;
    while i < n {
        let s = acc.saturating_add(y | 1);
        let d = acc.saturating_sub(y ^ 3);
        r ^= ((s as u64) ^ ((s >> 64) as u64).rotate_left(9)).rotate_left(i);
        r ^= ((d as u64) ^ ((d >> 64) as u64).rotate_left(5)).rotate_left(i.wrapping_add(7));
        acc = s ^ (d >> 1);
        i = i.wrapping_add(1);
    }
    r ^ (acc as u64)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b1 = ((input2 as u64) << 32) | input2 as u64;
    let a = (((a1 as u128) << 64) | b1 as u128) as i128;
    let b = (((b1 as u128) << 64) | a1 as u128) as i128;
    let s = a.saturating_add(b);
    let d = a.saturating_sub(b);
    let mut m = (s as u64) ^ ((s >> 64) as u64).rotate_left(9);
    m ^= ((d as u64) ^ ((d >> 64) as u64).rotate_left(5)).rotate_left(21);
    m ^= sat(a, b).rotate_left(3);
    m ^= sat(b, a ^ 1).rotate_left(11);
    m ^= sat_loop(a, b, (input2 & 7).wrapping_add(1)).rotate_left(17);
    (m as u32) ^ ((m >> 32) as u32)
}

// Campaign-12 lead: the guest LLVM `+wide-arithmetic` defect that reads the
// `i64.mul_wide_s` high word before it is written (F9, i64 checked
// multiplies) might also hit `i64.add128` / `i64.sub128`. This case runs
// u128/i128 `checked_add`, `overflowing_add`, `saturating_sub`,
// `checked_sub` and `overflowing_sub` inside `#[inline(never)]` helpers and
// a loop (the forms that expose F9 for multiplies), with both limbs of the
// operands at their boundaries on grid rows (all-ones low limbs make every
// carry ripple). No i64 checked/saturating multiply anywhere.
#[inline(never)]
fn checked(a: u128, b: u128) -> u128 {
    match a.checked_add(b) {
        Some(v) => v ^ 1,
        None => a.wrapping_sub(b) | 2,
    }
}

#[inline(never)]
fn overflowing(a: u128, b: u128) -> u128 {
    let (v, o) = a.overflowing_add(b);
    let (w, p) = a.overflowing_sub(b);
    v.rotate_left(3) ^ w ^ ((o as u128) << 127) ^ ((p as u128) << 64)
}

#[inline(never)]
fn signed(a: i128, b: i128) -> u128 {
    let s = a.saturating_sub(b);
    let c = a.checked_sub(b).map_or(0x7777_7777_7777_7777, |v| v ^ 3);
    let (d, o) = a.overflowing_add(b);
    (s as u128) ^ (c as u128).rotate_left(5) ^ (d as u128).rotate_left(9) ^ (o as u128)
}

#[inline(never)]
fn accumulate(seed: u128, step: u128, n: u32) -> u128 {
    let mut acc = seed;
    let mut i = 0u32;
    while i < n {
        acc = match acc.checked_add(step) {
            Some(v) => v,
            None => acc.wrapping_add(step) ^ (i as u128),
        };
        i = i.wrapping_add(1);
    }
    acc
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a1 = input1 as u64;
    let a3 = (a1 << 32) | a1;
    let b1 = input2 as u64;
    let b3 = (b1 << 32) | b1;
    // Operands with both limbs at their extremes on all-ones rows.
    let p: u128 = ((a3 as u128) << 64) | b3 as u128;
    let q: u128 = ((b3 as u128) << 64) | a3 as u128;
    let r: u128 = ((a1 as u128) << 96) | ((b1 as u128) << 32);
    let mut m = checked(p, q);
    m ^= checked(p, r).rotate_left(7);
    m ^= overflowing(q, p).rotate_left(11);
    m ^= overflowing(r, q).rotate_left(13);
    m ^= signed(p as i128, q as i128).rotate_left(17);
    m ^= signed((p >> 1) as i128, (q | (1 << 127)) as i128).rotate_left(19);
    m ^= accumulate(p, q | 1, input1 % 9).rotate_left(23);
    (m as u32) ^ ((m >> 32) as u32) ^ ((m >> 64) as u32) ^ ((m >> 96) as u32)
}

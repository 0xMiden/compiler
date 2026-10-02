// Nearest PASSING neighbours of the wide_words / wide_loop_cmp F9 shapes:
// wide results whose HIGH word alone is used (the low word dropped), whose
// low word alone is used (add128 keeps the op, mul_wide folds to a plain
// `i64.mul`), whose high word is compared against a CONSTANT (`hi != 0`,
// `hi < 0`), against the low word in a plain `if` (`hi < lo`), or selects a
// value (`if hi != lo >> k { x } else { lo }`), for `i64.mul_wide_u` /
// `mul_wide_s` / `add128` / `sub128` in `#[inline(never)]` helpers. None of
// these puts the multi-result op inside the second operand subtree of a
// binary op whose first operand is the high word, so LLVM stackifies them in
// a valid order: standalone builds agree with native at every opt-level and
// debuginfo level.
#[inline(never)]
fn mw_u_hi(x: u64, y: u64) -> u64 {
    ((x as u128).wrapping_mul(y as u128) >> 64) as u64
}

#[inline(never)]
fn mw_s_hi(x: i64, y: i64) -> u64 {
    ((x as i128).wrapping_mul(y as i128) >> 64) as u64
}

#[inline(never)]
fn add_hi(x: u128, y: u128) -> u64 {
    (x.wrapping_add(y) >> 64) as u64
}

#[inline(never)]
fn sub_hi(x: u128, y: u128) -> u64 {
    (x.wrapping_sub(y) >> 64) as u64
}

#[inline(never)]
fn add_lo(x: u128, y: u128) -> u64 {
    x.wrapping_add(y) as u64
}

#[inline(never)]
fn mw_s_ne0(x: i64, y: i64) -> u64 {
    let p = (x as i128).wrapping_mul(y as i128);
    let hi = (p >> 64) as i64;
    let lo = p as i64;
    if hi != 0 {
        return (lo as u64) ^ 1;
    }
    lo as u64
}

#[inline(never)]
fn mw_s_lt0(x: i64, y: i64) -> u64 {
    let p = (x as i128).wrapping_mul(y as i128);
    let hi = (p >> 64) as i64;
    let lo = p as i64;
    if hi < 0 {
        return (lo as u64) ^ 2;
    }
    (lo as u64) ^ (hi as u64)
}

#[inline(never)]
fn mw_u_lt(x: u64, y: u64) -> u64 {
    let p = (x as u128).wrapping_mul(y as u128);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    if hi < lo {
        return lo ^ 2;
    }
    lo ^ hi
}

#[inline(never)]
fn sub_lt(x: u128, y: u128) -> u64 {
    let p = x.wrapping_sub(y);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    if hi < lo {
        return lo ^ 2;
    }
    lo ^ hi
}

#[inline(never)]
fn mw_s_sel(x: i64, y: i64) -> u64 {
    let p = (x as i128).wrapping_mul(y as i128);
    let hi = (p >> 64) as i64;
    let lo = p as i64;
    if hi != (lo >> 63) {
        x as u64
    } else {
        lo as u64
    }
}

#[inline(never)]
fn add_sel(x: u128, y: u128) -> u64 {
    let p = x.wrapping_add(y);
    let hi = (p >> 64) as u64;
    let lo = p as u64;
    if hi != (lo >> 60) { x as u64 } else { lo }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input1 as u64 >> 3);
    let b = ((input2 as u64) << 32) | input2 as u64;
    let x = ((a as u128) << 64) | b as u128;
    let y = ((b as u128) << 64) | a as u128;
    let mut m = mw_u_hi(a, b) ^ mw_s_hi(a as i64, b as i64).rotate_left(3);
    m ^= add_hi(x, y).rotate_left(5) ^ sub_hi(x, y).rotate_left(7) ^ add_lo(x, y).rotate_left(9);
    m ^=
        mw_s_ne0(a as i64, b as i64).rotate_left(11) ^ mw_s_lt0(a as i64, b as i64).rotate_left(13);
    m ^= mw_u_lt(a, b).rotate_left(15) ^ sub_lt(x, y).rotate_left(17);
    m ^= mw_s_sel(a as i64, b as i64).rotate_left(19) ^ add_sel(x, y).rotate_left(23);
    (m as u32) ^ ((m >> 32) as u32)
}

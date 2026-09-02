// Comparison chains at sign and limb boundaries. The same 64-bit patterns
// a = (input1 << 32 | input2) and b = (input2 << 32 | input1) are compared
// signed (`::intrinsics::i64::lt/gt`, whose sign-difference arm decides
// without the u64 compare) and unsigned (`u64::lt/gt`) side by side, then
// through `Ord::cmp` on i64/i32/u32 (two strict compares selected into
// -1/0/1), i64 min/max/clamp and u64 max (compare + select). The
// `#[inline(never)]` helper compares i128 values that differ only in the low
// limb (high limbs equal: the `eq` + unsigned-low leg of the two-limb
// legalization) or by exactly one in the high limb (the signed-high leg,
// including the i64::MAX -> i64::MIN wrap of the high limb), plus i128 min
// and cmp. Rows (0x80000000, 0x7FFFFFFF) put a in the negative half and b in
// the positive half with mirrored magnitude bits.
#[inline(never)]
fn cmp128(a: i64, b: u64) -> u64 {
    let p: i128 = ((a as i128) << 64) | b as i128;
    let q: i128 = ((a as i128) << 64) | (b.rotate_left(1) as i128); // hi equal, lo differs
    let r: i128 = ((a.wrapping_add(1) as i128) << 64) | b as i128; // hi differs by one
    let mut acc: u64 = 0;
    if p < q {
        acc |= 1;
    }
    if p > r {
        acc |= 2;
    }
    if p == q {
        acc |= 4;
    }
    if r <= p {
        acc |= 8;
    }
    let mn = if p < r { p } else { r };
    let o = (p.cmp(&q) as i32) as u64 & 0xff;
    acc ^ (o << 8) ^ (mn as u64) ^ ((mn >> 64) as u64).rotate_left(5)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = (((input1 as u64) << 32) | input2 as u64) as i64;
    let b = (((input2 as u64) << 32) | input1 as u64) as i64;
    let ua = a as u64;
    let ub = b as u64;

    let mut acc: u32 = 0;
    if a < b {
        acc |= 1;
    }
    if ua < ub {
        acc |= 2;
    }
    if a > b {
        acc |= 4;
    }
    if ua > ub {
        acc |= 8;
    }
    acc |= ((a.cmp(&b) as i32) as u32 & 0xff) << 4;
    acc |= (((input1 as i32).cmp(&(input2 as i32)) as i32) as u32 & 0xff) << 12;
    acc |= ((input1.cmp(&input2) as i32) as u32 & 0xff) << 20;

    let mn = a.min(b);
    let mx = a.max(b);
    let cl = a.clamp(b.min(0), b.max(0));
    let um = ua.max(ub);
    let m = (mn as u64)
        ^ (mx as u64).rotate_left(7)
        ^ (cl as u64).rotate_left(13)
        ^ um.rotate_left(19)
        ^ cmp128(a, ub);
    acc.wrapping_add(m as u32).wrapping_add((m >> 32) as u32)
}

// F9 LADDER, one rung past the old boundary (campaign 31, 2026-09-17). The
// F9 family was "any both-limb value use of a wide-arithmetic op is
// miscompiled"; its passing neighbours only ever consumed the HIGH word, or a
// `hi != 0` flag, or were masked by guest DWARF. With the nightly-2026-09-01
// toolchain every F9 reproducer passes, so this case pushes the shape as far
// as plain Rust can: a loop that chains `i64.add128`, `i64.sub128`,
// `i64.mul_wide_u` and `i64.mul_wide_s` and consumes BOTH limbs of every
// result on every trip, feeding each one back into the next operation and
// into the loop's own exit test, with a saturating and a checked form of each
// so the overflow/borrow predicates are live too.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc: u128 = ((input1 as u128) << 64) | ((input2 as u128) << 32) | 1;
    let mut sacc: i128 = ((input2 as i128) << 64) ^ (input1 as i128);
    let mut flags: u64 = 0;
    let n = (input1 % 7) + 2;
    let mut i = 0u32;
    while i < n {
        let step = ((input2 as u128) << 32) | ((i as u128) + 1);

        // i64.add128 with both limbs used and the carry predicate live.
        let (sum, carried) = acc.overflowing_add(step);
        flags = flags.wrapping_mul(3) ^ (carried as u64);
        let sum_hi = (sum >> 64) as u64;
        let sum_lo = sum as u64;

        // i64.sub128 with both limbs used and the borrow predicate live.
        let (dif, borrowed) = sum.overflowing_sub(step ^ 0xffff_ffff);
        flags = flags.rotate_left(7) ^ (borrowed as u64);
        let dif_hi = (dif >> 64) as u64;
        let dif_lo = dif as u64;

        // i64.mul_wide_u with both words used.
        let prod = (sum_lo as u128).wrapping_mul(dif_lo as u128 | 1);
        let prod_hi = (prod >> 64) as u64;
        let prod_lo = prod as u64;

        // i64.mul_wide_s with both words used, through the signed accumulator.
        let sprod = (sacc as i128).wrapping_mul(((dif_hi as i64) | 1) as i128);
        let sprod_hi = (sprod >> 64) as i64;
        let sprod_lo = sprod as i64;

        // Saturating and checked forms, so the predicates stay live too.
        let sat = acc.saturating_add(prod);
        let chk = match acc.checked_mul(step | 1) {
            Some(v) => v,
            None => acc ^ prod,
        };
        let ssat = sacc.saturating_sub(sprod);

        acc = (((sum_hi ^ dif_hi ^ prod_hi) as u128) << 64)
            | ((sum_lo ^ dif_lo ^ prod_lo) as u128)
            ^ (sat >> 3)
            ^ (chk << 1);
        sacc = (((sprod_hi as u128) << 64) as i128) ^ (sprod_lo as i128) ^ (ssat >> 5);
        i = i.wrapping_add(1);
    }
    let a = ((acc >> 64) as u64) ^ (acc as u64);
    let s = (((sacc >> 64) as i64) ^ (sacc as i64)) as u64;
    ((a ^ s ^ flags) as u32) ^ (((a ^ s ^ flags) >> 32) as u32)
}

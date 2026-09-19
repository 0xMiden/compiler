// Mixed-width expression trees with casts in the middle (limb reassembly):
// `((a as u64 * b as u64) >> 32) as u32` (u64 product, high word by shift +
// truncation) XOR `((c as u128 * d as u128) >> 64) as u32` (mul_wide_u high
// limb truncated to its low word), u32 -> u64 -> u128 -> u64 -> u32 round
// trips with arithmetic at every width, a limb swap `(w as u32 as u64) << 32
// | (w >> 32)`, an i64 assembled from a signed high half and an unsigned low
// half, a u128 built from four u32 limbs and taken apart again with shifts
// by 32/64/96, and sums whose carry crosses the 32-bit limb inside a u64 and
// the 64-bit limb inside a u128 (rows with all-ones words make every carry
// ripple). The 128-bit part lives in an `#[inline(never)]` helper.
#[inline(never)]
fn tree128(c: u64, d: u64, e: u32) -> u64 {
    let prod: u128 = (c as u128) * (d as u128);
    let hi = (prod >> 64) as u32;
    let lo = prod as u32;
    let mid = (prod >> 32) as u64;
    let four: u128 =
        ((e as u128) << 96) | ((c as u32 as u128) << 64) | (((d >> 32) as u128) << 32) | e as u128;
    let l3 = (four >> 96) as u32;
    let l2 = (four >> 64) as u32;
    let l1 = (four >> 32) as u32;
    let l0 = four as u32;
    let sum = four.wrapping_add(((d as u128) << 32) | e as u128);
    let back = ((sum >> 64) as u64).wrapping_add(sum as u64);
    (hi ^ lo.rotate_left(3) ^ l3 ^ l2.rotate_left(7) ^ l1.rotate_left(11) ^ l0.rotate_left(13))
        as u64
        ^ mid.rotate_left(17)
        ^ back.rotate_left(23)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1;
    let b = input2;
    let w: u64 = ((a as u64) << 32) | b as u64;
    let c: u64 = w.rotate_left(17) ^ (b as u64).wrapping_mul(0x9E37_79B9);
    let d: u64 = ((b as u64) << 32) | a as u64;

    let t1 = ((a as u64 * b as u64) >> 32) as u32 ^ ((c as u128 * d as u128) >> 64) as u32;
    let round = ((((a as u64).wrapping_add(b as u64) as u128) << 64 | (w as u128)) >> 64) as u64;
    let round32 = (round.wrapping_mul(3) as u32).wrapping_add((round >> 32) as u32);
    let swapped: u64 = ((w as u32 as u64) << 32) | (w >> 32);
    let signed_hi: i64 = (((a as i32) as i64) << 32) | (b as i64);
    let carry64 = (a as u64 | 0xFFFF_FFFF_0000_0000).wrapping_add(b as u64); // crosses the u64 limb
    let m = swapped ^ (signed_hi as u64).rotate_left(5) ^ carry64.rotate_left(9) ^ tree128(c, d, a);
    t1.wrapping_add(round32.rotate_left(3))
        .wrapping_add(m as u32)
        .wrapping_add((m >> 32) as u32)
}

// F9 LADDER rung 2 (campaign 31, 2026-09-17): the `case_wide_limbs_chain`
// shape with FOUR u128 accumulators instead of one, all four live across the
// loop and all four consumed limb-by-limb in one joining expression per trip,
// plus three crossing rotate bands. Eight u64 limbs live across a
// bottom-tested loop is the freight range campaign 20/21 measured the spill
// cliff on, so this is where the wide-arithmetic ladder is expected to stop
// being about wide arithmetic.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut w0: u128 = ((input1 as u128) << 64) | (input2 as u128) | 1;
    let mut w1: u128 = ((input2 as u128) << 64) | (input1 as u128) | 3;
    let mut w2: u128 = ((input1 as u128) << 96) | ((input2 as u128) << 32) | 5;
    let mut w3: u128 = ((input2 as u128) << 96) | ((input1 as u128) << 32) | 7;
    let r0 = (input1 & 63) | 1;
    let r1 = (input2 & 63) | 2;
    let r2 = ((input1 ^ input2) & 63) | 4;
    let n = (input1 % 5) + 2;
    let mut i = 0u32;
    while i < n {
        let step = ((input2 as u128) << 32) | ((i as u128) + 1);

        let (s0, c0) = w0.overflowing_add(step);
        let (d1, b1) = w1.overflowing_sub(step ^ 0xffff_ffff);
        let p2 = (w2 as u64 as u128).wrapping_mul(((w3 as u64) | 1) as u128);
        let p3 = (w3 as u64 as u128).wrapping_mul(((w0 as u64) | 1) as u128);

        let h0 = (s0 >> 64) as u64;
        let l0 = s0 as u64;
        let h1 = (d1 >> 64) as u64;
        let l1 = d1 as u64;
        let h2 = (p2 >> 64) as u64;
        let l2 = p2 as u64;
        let h3 = (p3 >> 64) as u64;
        let l3 = p3 as u64;

        let mix = h0.rotate_left(r0)
            ^ l0.rotate_right(r1)
            ^ h1.rotate_left(r2)
            ^ l1.rotate_right(r0)
            ^ h2.rotate_left(r1)
            ^ l2.rotate_right(r2)
            ^ h3.rotate_left(r0)
            ^ l3.rotate_right(r1)
            ^ (c0 as u64)
            ^ (b1 as u64);

        w0 = (((h0 ^ mix) as u128) << 64) | (l0 as u128);
        w1 = (((h1 ^ mix.rotate_left(r1)) as u128) << 64) | (l1 as u128);
        w2 = (((h2 ^ mix.rotate_left(r2)) as u128) << 64) | (l2 as u128);
        w3 = (((h3 ^ mix.rotate_right(r0)) as u128) << 64) | (l3 as u128);
        i = i.wrapping_add(1);
    }
    let f = ((w0 >> 64) as u64)
        ^ (w0 as u64)
        ^ ((w1 >> 64) as u64)
        ^ (w1 as u64)
        ^ ((w2 >> 64) as u64)
        ^ (w2 as u64)
        ^ ((w3 >> 64) as u64)
        ^ (w3 as u64);
    (f as u32) ^ ((f >> 32) as u32)
}

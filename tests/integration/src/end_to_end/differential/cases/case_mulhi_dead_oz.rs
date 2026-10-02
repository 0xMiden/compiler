// Four `mulhi` rounds (the high half of a u128 product) under a shelf of
// eight live u32 values. LLVM lowers each to `i64.mul_wide_u`, whose LOW
// result is dead, so the block emitter's dead-instruction-result drop fires
// on a two-felt operand four times ("dropping dead instruction result %N at
// index 0"). Pinned at `--optimize=size-min`; the same source also passes at
// `--optimize=basic`/default/`--optimize=max`.
// deep shelf of live values at -Oz. Eight live u32s stay live across four
// mulhi rounds, so each dead 2-felt result is dropped from under eight live
// 1-felt operands (the `drop_operand_at_position` movup+drop arm on a
// multi-felt operand).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u64) << 32) | (input2 as u64) | 1;
    let b = (input2 as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 2;
    let v0 = input1 ^ 1;
    let v1 = input1 ^ 2;
    let v2 = input1 ^ 4;
    let v3 = input1 ^ 8;
    let v4 = input2 ^ 16;
    let v5 = input2 ^ 32;
    let v6 = input2 ^ 64;
    let v7 = input2 ^ 128;
    let h0 = (((a as u128) * (b as u128)) >> 64) as u64;
    let h1 = (((a.rotate_left(7) as u128) * (b as u128)) >> 64) as u64;
    let h2 = (((a as u128) * (b.rotate_left(13) as u128)) >> 64) as u64;
    let h3 = (((a.rotate_left(19) as u128) * (b.rotate_left(29) as u128)) >> 64) as u64;
    let mut acc = (h0 ^ h1 ^ h2 ^ h3) as u32;
    acc = acc.wrapping_add(v0).wrapping_add(v1).wrapping_add(v2).wrapping_add(v3);
    acc ^= v4 ^ v5 ^ v6 ^ v7;
    acc = acc.wrapping_add(((h0 ^ h3) >> 32) as u32);
    acc
}

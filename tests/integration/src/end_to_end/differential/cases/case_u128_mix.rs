// u128 arithmetic mixed into branchy code — exercises the wide-arithmetic
// wasm ops (`i64.add128`/`i64.sub128`/`i64.mul_wide_u`) in the frontend and
// their MASM lowering, with results feeding branch conditions.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = ((input1 as u128) << 64) | ((input2 as u128) << 17) | input1 as u128;
    let b = (input2 as u128).wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    let mut acc = a.wrapping_add(b);
    if acc & 1 == 0 {
        acc = acc.wrapping_sub(b.rotate_left(13));
    } else {
        acc = acc.wrapping_mul(b | 0x10);
    }
    let folded = (acc as u64) ^ ((acc >> 64) as u64);
    (folded as u32) ^ ((folded >> 32) as u32)
}

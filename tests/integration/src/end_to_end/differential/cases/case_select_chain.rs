// One reused condition feeding eight selects over two multi-use u64 values
// that both stay live past every select, with u64 freight computed before
// the selects and consumed after them: arity-3 `cf.select` problems whose
// condition and both arms are Copy-constrained, at increasing depth.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let c = (input1 ^ input2) & 1 == 0;
    let x = ((input1 ^ 0x85eb_ca6b) as u64) | 1;
    let y = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let f0 = x.wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let f1 = y.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let f2 = x.rotate_left(17) ^ y;
    let f3 = y.rotate_left(29) ^ x;
    let s0 = if c { x } else { y };
    let s1 = if c { y } else { x };
    let s2 = if c { x.rotate_left(3) } else { y.rotate_left(5) };
    let s3 = if c { y ^ 0x55 } else { x ^ 0xaa };
    let s4 = if c { x.wrapping_add(y) } else { x.wrapping_sub(y) };
    let s5 = if c { f0 } else { f1 };
    let s6 = if c { f2 } else { f3 };
    let s7 = if c { s0.rotate_left(1) } else { s1.rotate_left(2) };
    let r = s0 ^ s1.rotate_left(1) ^ s2.wrapping_add(s3) ^ s4.rotate_left(7) ^ s5 ^ s6.rotate_left(9) ^ s7
        ^ f0.rotate_left(11) ^ f1 ^ f2.rotate_left(13) ^ f3 ^ x ^ y;
    (r as u32) ^ ((r >> 32) as u32)
}

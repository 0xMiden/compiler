// A non-inlined helper returning u64 whose tail-merged return paths give
// the function-end block a 2-word (i64) block argument, plus an impossible
// trap exit and a multi-exit loop inside the helper. Branch successor
// operands are then multi-word values, exercising the word-count-aware
// operand scheduling in the cf/scf MASM lowerings.
#[inline(never)]
fn churn(a: u32, b: u32) -> u64 {
    let mut x = ((a as u64) << 32) | b as u64;
    if b & 0xf == 5 {
        return x.rotate_left(9) ^ 0x9e37_79b9_7f4a_7c15;
    }
    // Impossible: h % 6 == 1 implies h odd; h % 4 == 0 implies h even.
    let h = a ^ b.rotate_left(3);
    if h % 6 == 1 && h % 4 == 0 {
        panic!();
    }
    let mut i = 0u32;
    while i < (b % 37).wrapping_add(1) {
        x = x.wrapping_mul(0x0808_8405_0101_0193).wrapping_add(i as u64);
        if (x >> 60) == 0xa {
            return x ^ ((i as u64) << 17); // exit from inside the loop
        }
        i = i.wrapping_add(1);
    }
    x.rotate_right(23)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let r = churn(input1, input2);
    let s = churn(input2, input1.wrapping_add(0x55aa));
    let t = r ^ s.rotate_left(31);
    (t as u32) ^ ((t >> 32) as u32)
}

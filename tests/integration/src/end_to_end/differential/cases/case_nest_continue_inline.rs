// Inlined twin of `case_nest_continue.rs`: the same two `for` loops and
// `continue 'outer` with the helper `#[inline(always)]` — LLVM restructures
// the nest, no loop-invariant iter arg reaches the lifted `scf.while`, and
// the case compiles and passes.
#[inline(always)]
fn probe(v: u64, i: u32) -> u32 {
    (v.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(i & 63) >> 59) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut v = (input1 as u64) << 32 | input2 as u64;
    let mut skipped = 0u32;
    'outer: for i in 0..input2 % 5 {
        for j in 0..(input1 >> 4) % 4 {
            let p = probe(v, i ^ j);
            if p & 15 == 3 {
                skipped += 1;
                continue 'outer;
            }
            v = v.rotate_left(p) ^ j as u64;
        }
        v ^= 1 << (i & 63);
    }
    (v as u32) ^ ((v >> 32) as u32) ^ (skipped << 28)
}

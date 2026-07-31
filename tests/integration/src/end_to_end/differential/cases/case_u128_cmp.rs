// u128 comparisons over dynamic operand pairs: branch position (LLVM
// canonicalizes to strict two-limb compare chains: hi < hi | (hi == hi &
// lo < lo)) plus bool-VALUE forms in #[inline(never)] helpers — the only
// shape that keeps non-strict `<=` alive (KNOWLEDGE.md). u128_mix never
// compares two 128-bit values; u64_ucmp stops at single-limb u64 compares.
#[inline(never)]
fn le128(x: u128, y: u128) -> u32 {
    (x <= y) as u32
}

#[inline(never)]
fn eq128(x: u128, y: u128) -> u32 {
    (x == y) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a: u64 = ((input1 as u64) << 32) | input2 as u64;
    let b: u64 = (input2 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let p: u128 = ((a as u128) << 64) | b as u128;
    let q: u128 = ((b as u128) << 64) | a as u128;
    let r: u128 = p ^ (q >> 1);

    // Branch-position compares on distinct operand pairs (no predicate merging).
    let mut acc: u32 = 0;
    if p < q {
        acc = acc.wrapping_add(1);
    }
    if r > p {
        acc = acc.wrapping_add(2);
    }
    if q != r {
        acc = acc.wrapping_add(4);
    }
    // Select-position compare.
    let sel: u128 = if p <= r { p } else { r };

    // Bool-value compares (helper isolation keeps the non-strict form).
    acc = acc.wrapping_add(le128(q, p) << 3);
    acc = acc.wrapping_add(eq128(r, q) << 4);

    let m = (sel as u64) ^ ((sel >> 64) as u64);
    acc ^ (m as u32) ^ ((m >> 32) as u32)
}

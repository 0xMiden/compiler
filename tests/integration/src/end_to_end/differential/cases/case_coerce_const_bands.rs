// F18 LADDER rung 2 (campaign 31, 2026-09-17): the `case_coerce_const_fanout`
// fan-out repeated for TEN distinct literals inside a bottom-tested loop, so
// each literal is a count band crossing the loop AND is shared between the
// signed folder (an `i128` widening multiply), the truncating folder (a
// 64-bit rotate count) and a plain `i64` use. Ten crossing bands is the
// pressure range where campaign 21 measured the F6 cliff, so this is where
// the coercion ladder is expected to stop being about coercions.
const K: [u64; 10] = [7, 11, 13, 17, 19, 23, 29, 31, 37, 41];

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = (((input1 as u64) << 32) | (input2 as u64) | 1) as i64;
    let mut b = (((input2 as u64) << 32) | (input1 as u64) | 3) as i64;
    let mut c = ((input1 as u64) ^ ((input2 as u64) << 16)) | 5;
    let n = (input2 % 5) + 2;
    let mut i = 0u32;
    while i < n {
        let mut acc: i64 = 0;
        let mut j = 0usize;
        while j < 10 {
            let k = K[j];
            // Sext::fold — the literal widened for `i64.mul_wide_s`.
            let hi = (((a as i128).wrapping_mul(k as i128)) >> 64) as i64;
            // Trunc::fold — the same literal as a rotate count.
            let rot = c.rotate_left(k as u32) ^ c.rotate_right(k as u32);
            // Plain uses of the same literal at two widths.
            let p = b.wrapping_mul(k as i64).wrapping_add(k as i64);
            acc = acc.rotate_left(5) ^ hi ^ (rot as i64) ^ p;
            j += 1;
        }
        a = a.wrapping_add(acc) | 1;
        b = b ^ acc.rotate_right(11) | 3;
        c = c.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (acc as u64) | 5;
        i = i.wrapping_add(1);
    }
    let r = a ^ b ^ (c as i64);
    (r as u32) ^ ((r >> 32) as u32)
}

// Zero-trip and one-trip loop boundaries: two while-loops with loop-carried
// values whose trip counts come from `% 97` bounds LLVM cannot peel (range
// 0..=96). Edge relations asserted by the pinned grid: trip count exactly 0
// (the loop guard skips the rotated body entirely) and exactly 1, including
// the modulus wrap rows 97 -> 0 and 98 -> 1; the inner loop's 0..=2 trips are
// opportunistic (bound depends on the carried u64).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let t1 = input1 % 97; // trip count 0..=96
    let t2 = input2 % 97;

    // Loop 1: carried u32 accumulator.
    let mut acc = input2 | 1;
    let mut i = 0u32;
    while i < t1 {
        acc = acc.rotate_left(5).wrapping_add(input1 ^ i);
        i += 1;
    }

    // Loop 2: carried u64 with a nested 0..=2-trip inner loop.
    let mut w: u64 = ((input1 as u64) << 32) | input2 as u64;
    let mut j = 0u32;
    while j < t2 {
        w = w.rotate_right(9) ^ (j as u64).wrapping_mul(0x9e37_79b9);
        let lim = (w as u32) % 3;
        let mut k = 0u32;
        while k < lim {
            w = w.wrapping_add(0x0101_0101);
            k += 1;
        }
        j += 1;
    }

    acc ^ (w as u32) ^ ((w >> 32) as u32) ^ i ^ j
}

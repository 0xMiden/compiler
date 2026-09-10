// `slice::select_nth_unstable` on a runtime-length slice (campaign 27,
// Part A): the one member of `core`'s unstable-sort family that is still
// unusable from a guest. It reaches
// `core::slice::sort::select::median_of_medians`, which calls itself, and
// the Miden assembler rejects call-graph cycles. A CONSTANT length is not a
// reproducer — LLVM specialises the whole selection away — so the length
// here comes from the input.

fn seed(input1: u32, input2: u32) -> [u32; 64] {
    core::array::from_fn(|i| {
        let k = i as u32;
        input1.rotate_left(k & 31) ^ input2.wrapping_mul(k + 1)
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut a = seed(input1, input2);
    let n = 4 + (input2 % 60) as usize;
    let k = (input1 as usize) % n;
    let (lo, nth, hi) = a[..n].select_nth_unstable(k);
    let nth = *nth;
    let below = lo.iter().filter(|&&v| v > nth).count() as u32;
    let above = hi.iter().filter(|&&v| v < nth).count() as u32;
    nth.wrapping_add(below * 13).wrapping_add(above * 17)
}

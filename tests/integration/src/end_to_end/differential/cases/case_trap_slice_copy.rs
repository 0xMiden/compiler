// Trap parity: the two element-moving slice methods that panic. `swap(i, 5)`
// on a `[u32; 6]` panics for `i >= 6` (`input1 % 9` reaches 8), and
// `copy_from_slice` panics on a length mismatch — the destination length is
// `input2 % 7` and the source length is the same value except when
// `input2 % 4 == 0`, which perturbs it by one so the lengths differ. Both
// targets must trap on exactly those rows.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let src: [u32; 6] = [10, 20, 30, 40, 50, 60];
    let mut dst: [u32; 6] = [1, 2, 3, 4, 5, 6];
    let i = (input1 % 9) as usize;
    dst.swap(i, 5);
    let n = (input2 % 7) as usize;
    let m = if input2 % 4 == 0 { (n + 1) % 7 } else { n };
    dst[..n].copy_from_slice(&src[..m]);
    let mut acc = 0u32;
    let mut k = 0usize;
    while k < 6 {
        acc = acc.rotate_left(5) ^ dst[k];
        k += 1;
    }
    acc
}

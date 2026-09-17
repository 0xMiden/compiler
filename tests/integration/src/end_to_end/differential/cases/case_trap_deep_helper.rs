// Trap parity: a trap two `#[inline(never)]` frames down, in the SECOND of
// two calls to the same helper. The first call passes `input1 % 6`, which is
// always a valid index into `inner`'s `[u32; 6]`, so it returns normally and
// its result feeds the second call — which passes `input1` itself and traps
// whenever `input1 % 9 >= 6`. Both targets must trap on exactly those rows,
// after the first call's work.
#[inline(never)]
fn inner(x: u32) -> u32 {
    let table: [u32; 6] = [2, 3, 5, 7, 11, 13];
    table[(x % 9) as usize].wrapping_mul(x | 1)
}

#[inline(never)]
fn outer(x: u32, k: u32) -> u32 {
    inner(x).rotate_left(k % 32) ^ k
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let first = outer(input1 % 6, input2);
    let second = outer(input1, input2 ^ first);
    first ^ second
}

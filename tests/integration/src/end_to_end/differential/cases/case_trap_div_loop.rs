// Trap parity: a divisor that decrements to zero on a late loop trip. The
// divisor starts at `input2 % 10` and drops by one per trip over four trips,
// so the division traps exactly when `input2 % 10 < 4` — on trip
// `input2 % 10` — and returns above that, where it never reaches zero.
// The quotient of every earlier trip is folded into the accumulator, so the
// trap sits behind real work rather than at the function entry.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1 | 1;
    let mut d = input2 % 10;
    let mut i = 0u32;
    while i < 4 {
        acc = acc.wrapping_add(0x9e37_79b9) / d;
        d = d.wrapping_sub(1);
        i += 1;
    }
    acc
}

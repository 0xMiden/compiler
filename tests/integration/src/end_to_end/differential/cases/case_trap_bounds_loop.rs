// Trap parity: an index that walks off the end of a `[u32; 12]` part-way
// through a loop, so the trap happens only after several trips have already
// mixed values into the accumulator. `base = input1 % 20`, and trip `i`
// reads `buf[base + i]` for `i` in 0..5 — valid for `base <= 7`, trapping on
// the LAST trip at `base == 8` and on progressively earlier trips above it.
// The accumulated work before the trap cannot be observed; what must match
// is the trap-or-value decision itself.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let buf: [u32; 12] = [3, 1, 4, 1, 5, 9, 2, 6, 5, 3, 5, 8];
    let base = (input1 % 20) as usize;
    let mut acc = input2 ^ 0x9e37_79b9;
    let mut i = 0usize;
    while i < 5 {
        acc = acc.rotate_left(7).wrapping_add(buf[base + i]);
        i += 1;
    }
    acc
}

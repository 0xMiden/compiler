// Passing sibling of `case_invariant_args_min.rs`: the same nest, the same
// answer on every input, with the inner loop's early `return` moved BELOW the
// `break` instead of above it.  The lifted `scf.while` then has no payload
// column that is `ub.poison` on the continuing path, so
// `RemoveLoopInvariantArgsFromBeforeBlock` does not match and the program
// compiles.
const S1: u32 = 7;
const S2: u32 = 13;

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let len = ((input2 % 37) + 3) as usize;
    let mut x = input1 | 1;
    let mut sum = input1.rotate_left(S1);
    let mut pos = 0usize;
    while pos < len {
        loop {
            x ^= x << 13;
            x ^= x >> 17;
            let byte = (x >> 3) as u8;
            pos += 1;
            if byte & 0x80 == 0 {
                break;
            }
            if pos >= len {
                return (1u32 << 28) | (sum & 0x0fff_ffff);
            }
        }
        sum = sum.wrapping_add((pos as u32).rotate_left(S1));
    }
    let out = sum ^ (len as u32).rotate_left(S2);
    (5u32 << 28) | (out & 0x0fff_ffff)
}

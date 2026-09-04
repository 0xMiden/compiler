// Bare statically-infinite `loop {}` behind an impossible cross-modulus
// guard: the loop header is a block that contains only its own back-edge
// `cf.br`, so the passthrough-collapse canonicalizations that consider the
// guard's branch arms must take the collapse-into-self-loop bail instead of
// collapsing the branch (unreachable_exits keeps its infinite loop body
// non-empty on purpose, so this shape is not otherwise in the corpus).
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let h = input1 ^ input2.rotate_left(7);
    // h % 6 == 5 implies h % 3 == 2, contradicting h % 3 == 0.
    if h % 6 == 5 && h % 3 == 0 {
        loop {}
    }
    h.wrapping_mul(input2 | 1)
}

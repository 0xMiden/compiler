// MINIMAL COMPILE-TIME REPRODUCER of the F12 aliasing panic at the DEFAULT
// optimization level (campaign 22, reduced from `prog_varint`): an inner
// `loop` whose first statement is an early `return`, nested in an outer
// `while`, a two-step xorshift byte source, and two rotate constants: 7 used
// before and inside the loop, and 13 used only in the post-loop fold but
// shared with the xorshift's `<< 13` shift count.  Building it
// panics with `AliasingViolationError { kind: Mutable, location:
// hir/src/ir/operation.rs:877 }` at hir/src/patterns/rewriter.rs:335 while
// `RemoveLoopInvariantArgsFromBeforeBlock` rewrites.  See the ignored test in
// tests/compose.rs; `case_invariant_args_guard.rs` is the passing sibling
// that only moves the `return` below the `break`.
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
            if pos >= len {
                return (1u32 << 28) | (sum & 0x0fff_ffff);
            }
            x ^= x << 13;
            x ^= x >> 17;
            let byte = (x >> 3) as u8;
            pos += 1;
            if byte & 0x80 == 0 {
                break;
            }
        }
        sum = sum.wrapping_add((pos as u32).rotate_left(S1));
    }
    let out = sum ^ (len as u32).rotate_left(S2);
    (5u32 << 28) | (out & 0x0fff_ffff)
}

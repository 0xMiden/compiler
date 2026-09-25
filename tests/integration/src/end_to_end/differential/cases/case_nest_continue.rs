// COMPILE-TIME COMPILER PANIC REPRODUCER (campaign 14, 2026-09-03): two
// nested `for` loops, one `#[inline(never)]` call in the inner loop and a
// `continue 'outer` from the inner loop. The lifted outer `scf.while`
// carries a loop-invariant before-block argument, the
// `RemoveLoopInvariantArgsFromBeforeBlock` canonicalization fires and
// aborts with an `AliasingViolationError` at hir/src/patterns/rewriter.rs:335
// (it inlines the after region while its own borrow of the new while's
// region is alive). See the ignored test in tests/compose.rs;
// `case_nest_continue_inline.rs` is the passing inlined twin.
#[inline(never)]
fn probe(v: u64, i: u32) -> u32 {
    (v.wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(i & 63) >> 59) as u32
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut v = (input1 as u64) << 32 | input2 as u64;
    let mut skipped = 0u32;
    'outer: for i in 0..input2 % 5 {
        for j in 0..(input1 >> 4) % 4 {
            let p = probe(v, i ^ j);
            if p & 15 == 3 {
                skipped += 1;
                continue 'outer;
            }
            v = v.rotate_left(p) ^ j as u64;
        }
        v ^= 1 << (i & 63);
    }
    (v as u32) ^ ((v >> 32) as u32) ^ (skipped << 28)
}

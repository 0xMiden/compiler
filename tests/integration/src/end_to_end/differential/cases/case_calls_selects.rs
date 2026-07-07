// Non-inlined helper calls with varied signatures (multi-arg u32, u64
// params/results, bool) plus select-heavy code with reused results.
// Exercises `translate_call`, `hir::invoke::Exec` lowering,
// `OpEmitter::process_call_signature`, and the `select`/`dup_select`/
// `mov_select` emitter variants.

#[inline(never)]
fn mix(a: u32, b: u32, c: u32, d: u32) -> u32 {
    a.wrapping_mul(31).wrapping_add(b.rotate_left(c & 31)) ^ d
}

#[inline(never)]
fn wide(a: u64, b: u64) -> u64 {
    a.wrapping_mul(b | 1) ^ (a >> 7)
}

#[inline(never)]
fn pick(c: bool, x: u32, y: u32) -> u32 {
    if c { x } else { y }
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = mix(input1, input2, input1 >> 3, input2 ^ 0xa5a5_a5a5);
    let w = wide(((input1 as u64) << 32) | input2 as u64, input2 as u64 | 1);
    let p = pick(m & 1 == 0, m, input2);
    let q = pick(w as u32 & 2 == 0, w as u32, (w >> 32) as u32);
    // Reuse the selected values several times so the emitter has to dup/move
    // them on the operand stack.
    p.wrapping_add(q) ^ (p & q) ^ m
}

// Three dead count bands under SIX live-through ones (bands defined in
// alternating live/dead order before the loop) so that at
// `--optimize=size-min` the post-loop drop site sees fewer unused operands
// than used ones ("6 used operands out of 9"). That takes
// `drop_unused_operands_at`'s non-pathological branch -- the operand
// scheduler solves an all-`Move` problem and the emitter `dropn`s the result
// -- which no other case in the corpus reaches: `band_guard_oz` and
// `drop_batch_oz` both have more dead operands than live ones and take the
// manual interleave / whole-stack arms instead.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(28);
    acc = acc.wrapping_add(n.rotate_left(1));
    acc = acc.wrapping_sub(m.rotate_left(30));
    acc ^= n.rotate_left(3);
    acc = acc.wrapping_add(m.rotate_left(26));
    acc = acc.wrapping_sub(n.rotate_left(5));
    acc ^= m.rotate_left(24);
    acc = acc.wrapping_add(n.rotate_left(22));
    acc = acc.wrapping_sub(m.rotate_left(20));
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    while i < iters {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc = acc.wrapping_sub(acc.rotate_left(5));
        i = i.wrapping_add(1);
    }
    let mut out = acc ^ 0x5a5a_5a5a_5a5a_5a5a;
    out ^= acc.rotate_left(28);
    out = out.wrapping_add(out.rotate_left(30));
    out ^= acc.rotate_left(26);
    out = out.wrapping_add(out.rotate_left(24));
    out ^= acc.rotate_left(22);
    out = out.wrapping_add(out.rotate_left(20));
    (out as u32) ^ ((out >> 32) as u32)
}

// Passing guard at the -Oz count-band window boundary. Same shape as
// `case_spill_loop_mix.rs` (masked rotate counts shared between the pre-loop
// partner rotates of `m`/`n` and the rotates of the loop-carried `acc`, the
// 28/30 live-through pair, and a light second loop), but with NINE shared
// counts instead of sixteen: at `--optimize=size-min` LLVM keeps the count
// bands un-hoisted, so ten or more shared counts push the Copy-constrained
// count past the 16-felt window and the arity-2 scheduler panics with the
// known `NoSolution` (see the ignored `spill_loop_mix_oz`); nine is the
// largest count that compiles, and it passes differentially at -Oz and O2.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let m = (input1 | 1) as u64;
    let n = ((input2 ^ 0x9e37_79b9) as u64) | 2;
    let mut acc = (m ^ n) | 1;
    acc ^= m.rotate_left(1);
    acc = acc.wrapping_add(n.rotate_left(3));
    acc ^= m.rotate_left(5);
    acc = acc.wrapping_sub(n.rotate_left(7));
    acc ^= m.rotate_left(9);
    acc = acc.wrapping_add(n.rotate_left(11));
    acc ^= m.rotate_left(13);
    acc = acc.wrapping_sub(n.rotate_left(15));
    acc ^= m.rotate_left(17);
    acc ^= m.rotate_left(28);
    acc = acc.wrapping_add(n.rotate_left(30));
    let iters = (input2 % 97) + 3;
    let mut i: u32 = 0;
    while i < iters {
        acc ^= acc.rotate_left(1) | 1;
        acc = acc.wrapping_add(acc.rotate_left(3));
        acc ^= acc.rotate_left(5);
        acc = acc.wrapping_sub(acc.rotate_left(7));
        acc ^= acc.rotate_left(9);
        acc = acc.wrapping_add(acc.rotate_left(11));
        acc ^= acc.rotate_left(13);
        acc = acc.wrapping_sub(acc.rotate_left(15));
        acc ^= acc.rotate_left(17);
        i = i.wrapping_add(1);
    }
    let mut acc2 = acc | 1;
    let iters2 = (input1 % 89) + 2;
    let mut j: u32 = 0;
    while j < iters2 {
        acc2 ^= acc2.rotate_left(4);
        acc2 = acc2.wrapping_add(acc2.rotate_left(6));
        j = j.wrapping_add(1);
    }
    let r = acc2 ^ acc2.rotate_left(28) ^ acc.rotate_left(30);
    (r as u32) ^ ((r >> 32) as u32)
}

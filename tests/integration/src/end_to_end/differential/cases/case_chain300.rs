// SCALE DIMENSION: single-block operation count (~400 ops in ONE basic block).
// Four sequential "waves", each a right-leaning non-reassociable sub/rotate/xor
// tree (the case_stack_pressure recipe) at depth 24 (u32) / depth 12 (u64),
// joined by select-shaped combiners. At the innermost point of each wave ~26
// values (u32) / ~26 felts (u64) are simultaneously live — far past the
// 16-felt MASM window — so the block runs deep single-block spill/reload
// traffic and hundreds of operand-scheduling problems at every arity.
macro_rules! wave {
    ($x:expr, $y:expr;) => { $x.wrapping_mul($y | 3) };
    ($x:expr, $y:expr; $r:literal $(, $rest:literal)*) => {
        ($y ^ $x.rotate_left($r).wrapping_sub(wave!($y, $x; $($rest),*)))
            .rotate_right($r + 1)
    };
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = input1 | 1;
    let b = input2 ^ 0x9e37_79b9;

    // Wave 1: u32, depth 24.
    let w1 = wave!(a, b; 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24);
    // Select-shaped combiner (arity-3 scheduling with live values around it).
    let a1 = if w1 & 1 == 0 { a ^ w1 } else { a.wrapping_add(w1).rotate_left(3) };
    let b1 = if w1 & 2 == 0 { b.wrapping_sub(w1) } else { b ^ w1.rotate_right(5) };

    // Wave 2: u32, depth 24, different rotation schedule.
    let w2 = wave!(a1, b1; 25, 23, 21, 19, 17, 15, 13, 11, 9, 7, 5, 3, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24);
    let a2 = if w2 & 4 == 0 { a1.wrapping_add(w2) } else { a1 ^ w2.rotate_left(7) };

    // Wave 3: u64, depth 12 (24 felts of pending operands at the innermost point).
    let c = ((a2 as u64) << 32) | (b1 as u64);
    let d = ((w2 as u64) << 32) | (w1 as u64) | 5;
    let w3 = wave!(c, d; 3, 9, 15, 21, 27, 33, 39, 45, 51, 57, 61, 63);
    let m = if w3 & 8 == 0 { w3.rotate_left(11) } else { w3.wrapping_mul(0x2545_f491_4f6c_dd1d) };

    // Wave 4: u32, depth 24, seeded from the u64 wave's halves.
    let e = (m >> 32) as u32 | 1;
    let f = (m as u32).wrapping_add(a2);
    let w4 = wave!(e, f; 2, 5, 8, 11, 14, 17, 20, 23, 26, 29, 1, 4, 7, 10, 13, 16, 19, 22, 25, 28, 31, 3, 6, 9);

    w4.wrapping_add(w1 ^ w2).wrapping_sub(e).wrapping_mul(f | 1)
}

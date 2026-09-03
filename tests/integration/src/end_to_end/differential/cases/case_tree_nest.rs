// pressure_u64_32 x nest (campaign 14): six nested `while` loops with `% 3
// (+ 1)` bounds from different input bit fields — levels 1 and 4 are
// ZERO-TRIP-CAPABLE (guard + bypass edge) — whose innermost body is a
// right-leaning non-reassociable tree over twelve u64 leaves (24 felts in
// one block; the leaves mix the loop-carried `x` with twelve hoisted
// invariants), with an early `return` and a labeled `break` of level 1 from
// the innermost body and a `continue` of level 3 from level 6. Each level
// folds its own counter into the accumulator on the way out. Exit tag = top
// nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = [
        input2 % 3,
        ((input2 >> 2) % 3).wrapping_add(1),
        ((input2 >> 4) % 3).wrapping_add(1),
        (input2 >> 6) % 3,
        ((input1 >> 8) % 3).wrapping_add(1),
        ((input1 >> 10) % 3).wrapping_add(1),
    ];
    let b = ((input2 ^ 0x9e37_79b9) as u64).rotate_left(7) | 2;
    let mut x: u64 = (input1 | 1) as u64 ^ b.wrapping_mul(0x2545_f491_4f6c_dd1d);
    let mut tag = 1u32;
    let mut visits = 0u32;
    let mut i0 = 0u32;
    'l0: while i0 < n[0] {
        let mut i1 = 0u32;
        while i1 < n[1] {
            let mut i2 = 0u32;
            'l2: while i2 < n[2] {
                let mut i3 = 0u32;
                while i3 < n[3] {
                    let mut i4 = 0u32;
                    while i4 < n[4] {
                        let mut i5 = 0u32;
                        while i5 < n[5] {
                            visits = visits.wrapping_add(1);
                            let a = x ^ ((i5 as u64) << 40) ^ (i4 as u64);
                            x = a.wrapping_mul(0x9e3779b97f4a7c15).rotate_left(1)
                                ^ b.wrapping_add(0x0)
                                ^ (a.wrapping_mul(0x9e3779b97f4a7c17).rotate_left(8)
                                    ^ b.wrapping_add(0x123456789))
                                .wrapping_sub(
                                    (a.wrapping_mul(0x9e3779b97f4a7c19).rotate_left(15)
                                        ^ b.wrapping_add(0x2468acf12))
                                    .rotate_left(
                                        ((a.wrapping_mul(0x9e3779b97f4a7c1b).rotate_left(22)
                                            ^ b.wrapping_add(0x369d0369b)
                                            ^ (a.wrapping_mul(0x9e3779b97f4a7c1d).rotate_left(29)
                                                ^ b.wrapping_add(0x48d159e24))
                                            .wrapping_sub(
                                                (a.wrapping_mul(0x9e3779b97f4a7c1f)
                                                    .rotate_left(36)
                                                    ^ b.wrapping_add(0x5b05b05ad))
                                                .rotate_left(
                                                    ((a.wrapping_mul(0x9e3779b97f4a7c21)
                                                        .rotate_left(43)
                                                        ^ b.wrapping_add(0x6d3a06d36)
                                                        ^ (a.wrapping_mul(0x9e3779b97f4a7c23)
                                                            .rotate_left(50)
                                                            ^ b.wrapping_add(0x7f6e5d4bf))
                                                        .wrapping_sub(
                                                            (a.wrapping_mul(0x9e3779b97f4a7c25)
                                                                .rotate_left(57)
                                                                ^ b.wrapping_add(0x91a2b3c48))
                                                            .rotate_left(
                                                                ((a.wrapping_mul(
                                                                    0x9e3779b97f4a7c27,
                                                                )
                                                                .rotate_left(2)
                                                                    ^ b.wrapping_add(0xa3d70a3d1)
                                                                    ^ (a.wrapping_mul(
                                                                        0x9e3779b97f4a7c29,
                                                                    )
                                                                    .rotate_left(9)
                                                                        ^ b.wrapping_add(
                                                                            0xb60b60b5a,
                                                                        ))
                                                                    .wrapping_sub(
                                                                        a.wrapping_mul(
                                                                            0x9e3779b97f4a7c2b,
                                                                        )
                                                                        .rotate_left(16)
                                                                            ^ b.wrapping_add(
                                                                                0xc83fb72e3,
                                                                            ),
                                                                    ))
                                                                    as u32
                                                                    & 31),
                                                            ),
                                                        ))
                                                        as u32
                                                        & 31),
                                                ),
                                            )) as u32
                                            & 31),
                                    ),
                                );
                            if x & 0x3ff == 0x111 {
                                return (5 << 28) | (((x as u32) ^ visits) & 0x0fff_ffff);
                            }
                            if x & 0x1ff == 0x22 {
                                tag = 3;
                                x ^= i5 as u64;
                                break 'l0;
                            }
                            i5 = i5.wrapping_add(1);
                        }
                        x = x.wrapping_add(i5 as u64);
                        if x & 0xf00 == 0x500 {
                            tag = 2; // overwritten by any later break
                            x = x.rotate_left(3) ^ (i4 as u64);
                            i2 = i2.wrapping_add(1);
                            continue 'l2;
                        }
                        i4 = i4.wrapping_add(1);
                    }
                    x ^= (i4 as u64) << 12;
                    i3 = i3.wrapping_add(1);
                }
                x = x.wrapping_add((i3 as u64) << 16);
                i2 = i2.wrapping_add(1);
            }
            x ^= (i2 as u64) << 20;
            i1 = i1.wrapping_add(1);
        }
        x = x.wrapping_add((i1 as u64) << 24);
        i0 = i0.wrapping_add(1);
    }
    (tag << 28)
        | (((x as u32) ^ ((x >> 32) as u32) ^ visits.wrapping_mul(0x9e37_79b9) ^ i0) & 0x0fff_ffff)
}

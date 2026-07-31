// SCALE DIMENSION: control-flow nesting depth. Twelve levels of mixed
// while-loops and conditionals (loops at levels 1/3/6/9 with %17/%13-derived
// dynamic trip counts that resist LLVM peeling; conditionals elsewhere), with
// the accumulator and every loop counter threaded through all levels —
// cfg-to-scf structural recursion, scf region nesting, and lowering at a
// depth the corpus never reached. Worst case ~900 innermost iterations.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut acc = input1 ^ 0x0bad_c0de;
    let n1 = input1 % 17;
    let n2 = input2 % 13;

    let mut i = 0u32;
    // L1: loop
    while i < n1 {
        // L2: if
        if (acc ^ i) & 1 == 0 {
            // L3: loop
            let mut j = 0u32;
            while j < n2 {
                // L4: if
                if (acc >> 1) & 1 == 0 {
                    // L5: if
                    if (i.wrapping_add(j)) & 1 == 0 {
                        // L6: loop
                        let mut k = 0u32;
                        while k < 2 {
                            // L7: if
                            if acc & 4 == 0 {
                                // L8: if
                                if acc & 8 == 0 {
                                    // L9: loop
                                    let mut l = 0u32;
                                    while l < 2 {
                                        // L10: if
                                        if acc & 16 == 0 {
                                            // L11: if
                                            if acc & 32 == 0 {
                                                // L12: if
                                                if acc & 64 == 0 {
                                                    acc = acc
                                                        .wrapping_mul(2654435761)
                                                        .rotate_left(5);
                                                } else {
                                                    acc = acc.wrapping_add(0x1111);
                                                }
                                            } else {
                                                acc ^= 0x2222;
                                            }
                                        } else {
                                            acc = acc.rotate_right(3).wrapping_sub(l);
                                        }
                                        l += 1;
                                    }
                                } else {
                                    acc = acc.wrapping_add(i ^ j.rotate_left(3));
                                }
                            } else {
                                acc ^= k.wrapping_mul(97).wrapping_add(0x3333);
                            }
                            k += 1;
                        }
                    } else {
                        acc = acc.rotate_left(7) ^ j;
                    }
                } else {
                    acc = acc.wrapping_sub(19).rotate_right(1);
                }
                j += 1;
            }
        } else {
            acc = acc.wrapping_add(41).rotate_right(2);
        }
        i += 1;
    }
    acc.wrapping_add(n1.wrapping_mul(64).wrapping_add(n2))
}

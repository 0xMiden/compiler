// Eight nested `while` loops with `% 3 (+ 1)` bounds taken from different
// input bit fields — levels 1, 4 and 8 are ZERO-TRIP-CAPABLE (LLVM keeps a
// guard + bypass edge there), the others run one to three trips — with
// escapes at four different depths: a `continue` of level 3 from level 6,
// a labeled break of level 5 from level 8, a labeled break of level 1 from
// level 7, and an early return from level 8. Each level folds its own
// counter into the accumulator on the way out so a stale or swapped
// counter changes the result. Exit tag = top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = [
        input2 % 3,
        ((input2 >> 2) % 3).wrapping_add(1),
        ((input2 >> 4) % 3).wrapping_add(1),
        (input2 >> 6) % 3,
        ((input1 >> 8) % 3).wrapping_add(1),
        ((input1 >> 10) % 3).wrapping_add(1),
        ((input1 >> 12) % 3).wrapping_add(1),
        (input1 >> 14) % 3,
    ];
    let mut x = input1 ^ input2.rotate_left(13);
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
                    'l4: while i4 < n[4] {
                        let mut i5 = 0u32;
                        while i5 < n[5] {
                            let mut i6 = 0u32;
                            while i6 < n[6] {
                                let mut i7 = 0u32;
                                while i7 < n[7] {
                                    visits = visits.wrapping_add(1);
                                    x = x.wrapping_mul(0x0808_8405).wrapping_add(i7 ^ (i6 << 2));
                                    if x & 0x3ff == 0x111 {
                                        return (5 << 28) | ((x ^ visits) & 0x0fff_ffff);
                                    }
                                    if x & 0x1ff == 0x22 {
                                        tag = 4;
                                        x ^= i7;
                                        break 'l4;
                                    }
                                    i7 = i7.wrapping_add(1);
                                }
                                x = x.wrapping_add(i7);
                                if x & 0xff0 == 0x300 {
                                    tag = 3;
                                    x = x.rotate_left(1) ^ i6;
                                    break 'l0;
                                }
                                i6 = i6.wrapping_add(1);
                            }
                            x ^= i6 << 4;
                            if x & 0xf00 == 0x500 {
                                tag = 2; // overwritten by any later break
                                x = x.wrapping_add(i5);
                                i2 = i2.wrapping_add(1);
                                continue 'l2;
                            }
                            i5 = i5.wrapping_add(1);
                        }
                        x = x.wrapping_add(i5 << 8);
                        i4 = i4.wrapping_add(1);
                    }
                    x ^= i4 << 12;
                    i3 = i3.wrapping_add(1);
                }
                x = x.wrapping_add(i3 << 16);
                i2 = i2.wrapping_add(1);
            }
            x ^= i2 << 20;
            i1 = i1.wrapping_add(1);
        }
        x = x.wrapping_add(i1 << 24);
        i0 = i0.wrapping_add(1);
    }
    (tag << 28) | ((x ^ visits.wrapping_mul(0x9e37_79b9) ^ i0) & 0x0fff_ffff)
}

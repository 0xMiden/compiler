// Five nested zero-trip-capable loops whose innermost body can leave to
// EVERY enclosing level (labeled breaks to levels 1-4, a plain break, a
// `continue` of level 2, and an early return), and whose per-level tails
// each carry one more labeled break — many exit-dispatch columns at a
// shallow depth. Exit tag = top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let n = [
        input2 % 3,
        (input2 >> 2) % 3,
        (input2 >> 4) % 3,
        (input1 >> 2) % 3,
        (input1 >> 4) % 3,
    ];
    let mut x = input1 ^ input2.rotate_left(13);
    let mut tag = 1u32;
    let mut visits = 0u32;
    let mut i0 = 0u32;
    'l0: while i0 < n[0] {
        let mut i1 = 0u32;
        'l1: while i1 < n[1] {
            let mut i2 = 0u32;
            'l2: while i2 < n[2] {
                let mut i3 = 0u32;
                'l3: while i3 < n[3] {
                    let mut i4 = 0u32;
                    while i4 < n[4] {
                        visits = visits.wrapping_add(1);
                        x = x.wrapping_mul(0x0808_8405).wrapping_add(i4 ^ (i3 << 2) ^ (i2 << 4));
                        match x & 0x3ff {
                            0x111 => return (9 << 28) | ((x ^ visits) & 0x0fff_ffff),
                            0x022 => {
                                tag = 8;
                                x ^= i4;
                                break;
                            }
                            0x033 => {
                                tag = 7;
                                x ^= i3;
                                break 'l3;
                            }
                            0x044 => {
                                tag = 6;
                                x ^= i2;
                                break 'l2;
                            }
                            0x055 => {
                                tag = 5;
                                x ^= i1;
                                break 'l1;
                            }
                            0x066 => {
                                tag = 4;
                                x ^= i0;
                                break 'l0;
                            }
                            0x077 => {
                                tag = 3;
                                i2 = i2.wrapping_add(1);
                                continue 'l2;
                            }
                            _ => {}
                        }
                        i4 = i4.wrapping_add(1);
                    }
                    x = x.wrapping_add(i4);
                    if x & 0xff0 == 0x300 {
                        tag = 2;
                        break 'l1;
                    }
                    i3 = i3.wrapping_add(1);
                }
                x = x.wrapping_add(i3 << 4);
                if x & 0xff00 == 0x5500 {
                    tag = 10;
                    break 'l0;
                }
                i2 = i2.wrapping_add(1);
            }
            x = x.wrapping_add(i2 << 8);
            if x & 0xf_0000 == 0x7_0000 {
                tag = 11;
                break 'l0;
            }
            i1 = i1.wrapping_add(1);
        }
        x = x.wrapping_add(i1 << 12);
        i0 = i0.wrapping_add(1);
    }
    (tag << 28) | ((x ^ visits.wrapping_mul(0x9e37_79b9) ^ i0) & 0x0fff_ffff)
}

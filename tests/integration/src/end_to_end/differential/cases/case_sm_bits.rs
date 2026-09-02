// Eight-state machine driven by one input bit per step (input1 is rotated
// one bit per transition, so the bit stream never runs out), with four
// exits — three `break` sites in different arms plus a step-budget exit at
// the loop header — two `continue` arms that skip the shared tail, and a
// shared tail every other arm falls into. The exit tag lands in the top
// nibble of the result so a pinned grid can name the exit each input takes.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut bits = input1;
    let mut acc = input2;
    let mut state = 0u32;
    let mut steps = 0u32;
    let tag = loop {
        if steps >= 40 {
            break 9u32; // exit T: step budget exhausted (header exit)
        }
        steps = steps.wrapping_add(1);
        let bit = bits & 1;
        bits = bits.rotate_right(1);
        match state {
            0 => {
                state = if bit == 1 { 1 } else { 2 };
                acc = acc.wrapping_add(0x11);
            }
            1 => {
                if bit == 1 {
                    state = 3;
                    continue; // skips the shared tail
                }
                state = 4;
                acc ^= 0x2222;
            }
            2 => {
                acc = acc.rotate_left(3);
                state = if bit == 1 { 5 } else { 0 };
            }
            3 => {
                if acc & 7 == 5 {
                    break 3; // exit A
                }
                state = 6;
                acc = acc.wrapping_mul(5);
            }
            4 => {
                state = 7;
                if bit == 0 {
                    continue; // skips the shared tail
                }
                acc = acc.wrapping_sub(0x33);
            }
            5 => {
                if bit == 1 && acc & 0x10 != 0 {
                    break 5; // exit B
                }
                state = 1;
            }
            6 => {
                acc ^= bits;
                state = if bit == 1 { 2 } else { 7 };
            }
            _ => {
                if steps > 20 && bit == 1 {
                    break 7; // exit C
                }
                state = 0;
                acc = acc.wrapping_add(bits);
            }
        }
        // Shared tail.
        acc = acc.wrapping_add(state);
    };
    (tag << 28) | ((acc ^ state ^ steps.wrapping_mul(0x0101_0101)) & 0x0fff_ffff)
}

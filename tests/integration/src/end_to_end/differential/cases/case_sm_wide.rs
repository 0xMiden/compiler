// State machines whose state is not a small u32: a u64 state matched
// against 64-bit constants (LLVM lowers the switch as a br_table on a
// wrapped half of the state plus i64 equality compares), and a (u32, u32)
// pair state matched with tuple patterns and guards. Both consume one input bit per step and stop
// on a step budget or a state-specific break; the exit tags of both
// machines are packed into the top nibble.

// A: u64 state with 64-bit state constants.
#[inline(never)]
fn sm_u64(input1: u32, input2: u32) -> u32 {
    const S0: u64 = 0;
    const S1: u64 = 1 << 32;
    const S2: u64 = (2 << 32) | 7;
    const S3: u64 = 3 << 32;
    const S4: u64 = 0xffff_ffff_0000_0004;
    const S5: u64 = (5 << 32) | 0x8000_0000;
    let mut bits = input1;
    let mut acc = input2 as u64 ^ 0x9e37_79b9_7f4a_7c15;
    let mut state = S0;
    let mut steps = 0u32;
    let tag = loop {
        if steps >= 33 {
            break 1u32;
        }
        steps = steps.wrapping_add(1);
        let bit = bits & 1;
        bits = bits.rotate_right(1);
        match state {
            S0 => {
                state = if bit == 1 { S1 } else { S2 };
                acc = acc.wrapping_add(0x11);
            }
            S1 => {
                if bit == 1 && acc & 0x30 == 0x20 {
                    break 2; // exit A
                }
                state = S3;
                acc ^= 0x2222;
            }
            S2 => {
                acc = acc.rotate_left(3);
                state = if bit == 1 { S4 } else { S0 };
            }
            S3 => {
                state = if bit == 1 { S5 } else { S2 };
                acc = acc.wrapping_mul(5);
                continue;
            }
            S4 => {
                if acc & 7 == 5 {
                    break 3; // exit B
                }
                state = S1;
            }
            S5 => {
                state = if bit == 1 { S0 } else { S4 };
                acc = acc.wrapping_sub(bits as u64);
            }
            _ => {
                break 4; // unreachable by construction (only S0..S5 assigned)
            }
        }
        acc ^= state >> 29;
    };
    (tag << 28) | (((acc ^ (acc >> 32)) as u32 ^ steps) & 0x0fff_ffff)
}

// B: (u32, u32) pair state with tuple patterns and guards.
#[inline(never)]
fn sm_pair(input1: u32, input2: u32) -> u32 {
    let mut bits = input2;
    let mut acc = input1;
    let mut state = (0u32, 0u32);
    let mut steps = 0u32;
    let tag = loop {
        if steps >= 37 {
            break 1u32;
        }
        steps = steps.wrapping_add(1);
        let bit = bits & 1;
        bits = bits.rotate_left(1);
        state = match state {
            (0, _) => {
                acc = acc.wrapping_add(bit);
                (1, bit)
            }
            (1, 0) => {
                acc ^= 0x55;
                (2, 0)
            }
            (1, x) => {
                acc = acc.rotate_left(x.wrapping_add(1));
                (2, x.wrapping_add(1))
            }
            (2, x) if x > 3 => {
                if acc & 0x0f == 0x0a {
                    break 2; // exit A
                }
                (0, 0)
            }
            (2, x) => {
                acc = acc.wrapping_mul(3) ^ x;
                if bit == 1 {
                    (3, x)
                } else {
                    (2, x.wrapping_add(2))
                }
            }
            (3, x) => {
                if x == 2 && bit == 0 {
                    break 3; // exit B
                }
                acc = acc.wrapping_sub(x);
                (0, x)
            }
            _ => break 4, // unreachable by construction
        };
        acc = acc.wrapping_add(state.0 ^ state.1);
    };
    (tag << 28) | ((acc ^ state.0.wrapping_mul(0x0101_0101) ^ state.1 ^ steps) & 0x0fff_ffff)
}

#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let a = sm_u64(input1, input2);
    let b = sm_pair(input1, input2);
    ((a >> 28) << 30) | ((b >> 28) << 28) | ((a ^ b.rotate_left(7)) & 0x0fff_ffff)
}

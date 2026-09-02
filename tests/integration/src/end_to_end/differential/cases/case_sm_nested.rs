// Two nested state machines: every step of the outer machine runs the inner
// machine to completion on the next bits of the stream, and the inner
// machine's exit tag selects the outer transition. Exits: the outer machine
// breaks from two different arms or on its step budget; the inner machine
// breaks from two arms or on its own budget. The outer exit tag sits in the
// top nibble.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let mut bits = input1;
    let mut acc = input2;
    let mut outer = 0u32;
    let mut outer_steps = 0u32;
    let tag = loop {
        if outer_steps >= 12 {
            break 1u32; // outer exit T
        }
        outer_steps = outer_steps.wrapping_add(1);

        // Inner machine: consumes bits until it exits.
        let mut inner = outer & 1;
        let mut inner_steps = 0u32;
        let inner_tag = loop {
            if inner_steps >= 5 {
                break 0u32; // inner exit T
            }
            inner_steps = inner_steps.wrapping_add(1);
            let bit = bits & 1;
            bits = bits.rotate_right(1);
            match inner {
                0 => {
                    acc = acc.wrapping_add(0x101);
                    inner = if bit == 1 { 1 } else { 2 };
                }
                1 => {
                    if bit == 1 {
                        break 1; // inner exit A
                    }
                    acc ^= 0x0f0f;
                    inner = 3;
                }
                2 => {
                    acc = acc.rotate_left(1);
                    if acc & 0x18 == 0x08 {
                        break 2; // inner exit B
                    }
                    inner = if bit == 1 { 0 } else { 3 };
                }
                _ => {
                    acc = acc.wrapping_mul(9) ^ bit;
                    inner = 0;
                    continue; // skip the inner tail
                }
            }
            acc = acc.wrapping_add(inner);
        };

        match (outer, inner_tag) {
            (0, 0) => outer = 1,
            (0, 1) => outer = 2,
            (0, _) => {
                acc ^= 0x1234;
                outer = 3;
            }
            (1, 1) => {
                if acc & 0x7 == 0x3 {
                    break 2; // outer exit A
                }
                outer = 0;
            }
            (1, _) => outer = 2,
            (2, 2) => {
                if outer_steps > 3 {
                    break 3; // outer exit B
                }
                outer = 1;
            }
            (2, _) => {
                acc = acc.wrapping_sub(inner_steps);
                outer = 3;
            }
            _ => outer = 0,
        }
        acc = acc.wrapping_add(outer.wrapping_mul(0x0100_0100) ^ inner_tag);
    };
    (tag << 28) | ((acc ^ outer ^ outer_steps.rotate_left(9)) & 0x0fff_ffff)
}

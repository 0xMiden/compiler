// Nested labelled loops with local and non-local exits.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let outer_limit = (input1 & 3).wrapping_add(2);
    let inner_limit = (input2 & 3).wrapping_add(1);
    let mut outer = 0;
    let mut acc = input1.wrapping_add(input2 ^ 0x3141_5926);

    let result = 'outer: loop {
        if outer >= outer_limit {
            break acc ^ outer;
        }

        let mut inner = 0;
        loop {
            if inner >= inner_limit {
                break;
            }

            let mix = acc ^ inner ^ outer;
            if (mix & 7) == 3 {
                break 'outer acc.wrapping_add(inner).wrapping_add(outer);
            }

            if (mix & 1) == 0 {
                acc = acc.wrapping_add(input1 ^ inner);
            } else {
                acc = acc.wrapping_sub(input2 ^ outer);
            }

            inner = inner.wrapping_add(1);
        }

        outer = outer.wrapping_add(1);
        acc = acc.wrapping_add(outer ^ inner_limit);
    };

    result.wrapping_add(acc & 0x0000_ffff)
}

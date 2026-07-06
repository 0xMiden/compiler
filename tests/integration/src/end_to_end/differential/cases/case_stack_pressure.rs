// A right-leaning, non-reassociable expression tree. Every intermediate is
// used exactly once, so LLVM stackifies the whole tree onto the wasm operand
// stack; at the innermost point ~20 values are simultaneously live, pushing
// the MASM operand stack past its 16-slot limit and exercising the spill
// analysis/transform and deep operand-scheduling paths.
#[unsafe(no_mangle)]
pub extern "C" fn entrypoint(input1: u32, input2: u32) -> u32 {
    let x = input1 | 1;
    let y = input2 ^ 0x9e37_79b9;
    x.wrapping_sub(
        (y ^ x.rotate_left(1).wrapping_sub(
            (x ^ y.rotate_left(2).wrapping_sub(
                (y ^ x.rotate_left(3).wrapping_sub(
                    (x ^ y.rotate_left(4).wrapping_sub(
                        (y ^ x.rotate_left(5).wrapping_sub(
                            (x ^ y.rotate_left(6).wrapping_sub(
                                (y ^ x.rotate_left(7).wrapping_sub(
                                    (x ^ y.rotate_left(8).wrapping_sub(
                                        (y ^ x.rotate_left(9)
                                            .wrapping_sub(x.wrapping_mul(y | 3)))
                                        .rotate_right(3),
                                    ))
                                    .rotate_right(5),
                                ))
                                .rotate_right(7),
                            ))
                            .rotate_right(9),
                        ))
                        .rotate_right(11),
                    ))
                    .rotate_right(13),
                ))
                .rotate_right(15),
            ))
            .rotate_right(17),
        ))
        .rotate_right(19),
    )
}

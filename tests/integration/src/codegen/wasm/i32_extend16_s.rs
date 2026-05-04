use super::*;

#[test]
fn i32_extend16_s() {
    let span = SourceSpan::default();

    let (package, context) = compile_test_module([Type::I32], [Type::I32], |builder| {
        let block = builder.current_block();
        let input = block.borrow().arguments()[0] as ValueRef;
        let result = builder.sign_extend(input, Type::I16, Type::I32, span).unwrap();
        builder.ret(Some(result), span).unwrap();
    });

    // (input, expected_out)
    let cases: Vec<(u32, u32)> = Vec::from([
        (
            0b0000_0000_0000_0000_0000_0000_0000_0000,
            0b0000_0000_0000_0000_0000_0000_0000_0000,
        ),
        (
            0b0000_0000_0000_0000_0000_0000_0000_0001,
            0b0000_0000_0000_0000_0000_0000_0000_0001,
        ),
        (
            0b0000_0000_0000_0000_0111_1111_1111_1111,
            0b0000_0000_0000_0000_0111_1111_1111_1111,
        ),
        (
            0b0000_0000_0000_0000_1000_0000_0000_0000,
            0b1111_1111_1111_1111_1000_0000_0000_0000,
        ),
        (
            0b0000_0000_0000_0000_1111_1111_1111_1111,
            0b1111_1111_1111_1111_1111_1111_1111_1111,
        ),
        (
            0b0000_0000_0000_0000_1000_0000_0000_0001,
            0b1111_1111_1111_1111_1000_0000_0000_0001,
        ),
        (
            0b0001_0010_0011_0100_1000_0000_0000_0000,
            0b1111_1111_1111_1111_1000_0000_0000_0000,
        ),
        (
            0b1010_1011_1100_1101_0111_1111_1111_1111,
            0b0000_0000_0000_0000_0111_1111_1111_1111,
        ),
        (
            0b1111_1111_1111_1111_1111_1111_1111_1111,
            0b1111_1111_1111_1111_1111_1111_1111_1111,
        ),
        (
            0b1111_1111_1111_1111_0000_0000_0000_0000,
            0b0000_0000_0000_0000_0000_0000_0000_0000,
        ),
    ]);

    for (input, expected_out) in cases {
        assert_eq!(((input as i16) as i32) as u32, expected_out, "invalid test case");

        eval_package::<u32, _, _>(
            &package,
            None,
            &[Felt::from(input)],
            context.session(),
            |trace| {
                let outputs = trace.outputs().as_int_vec();
                assert_single_output(expected_out as u64, outputs);
                Ok(())
            },
        )
        .unwrap();
    }
}

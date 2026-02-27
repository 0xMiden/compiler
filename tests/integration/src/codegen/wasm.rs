use midenc_dialect_wasm::WasmOpBuilder;
use midenc_hir::{
    AbiParam, Felt, Signature, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder,
};

use crate::testing::{compile_test_module, eval_package};

#[test]
#[ignore = "https://github.com/0xMiden/compiler/pull/986#discussion_r2859923681"]
fn test_i32_extend8_s() {
    let span = SourceSpan::default();
    let signature = Signature::new([AbiParam::new(Type::I32)], [AbiParam::new(Type::I32)]);

    let (package, context) = compile_test_module(signature, |builder| {
        let block = builder.current_block();
        let input = block.borrow().arguments()[0] as ValueRef;
        let result = builder.i32_extend8_s(input, span).unwrap();
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
            0b0000_0000_0000_0000_0000_0000_0111_1111,
            0b0000_0000_0000_0000_0000_0000_0111_1111,
        ),
        (
            0b0000_0000_0000_0000_0000_0000_1000_0000,
            0b1111_1111_1111_1111_1111_1111_1000_0000,
        ),
        (
            0b0000_0000_0000_0000_0000_0000_1111_1111,
            0b1111_1111_1111_1111_1111_1111_1111_1111,
        ),
        (
            0b0000_0000_0000_0000_0000_0000_1000_0001,
            0b1111_1111_1111_1111_1111_1111_1000_0001,
        ),
        (
            0b0001_0010_0011_0100_0101_0110_1000_0000,
            0b1111_1111_1111_1111_1111_1111_1000_0000,
        ),
        (
            0b1010_1011_1100_1101_1110_1111_0111_1111,
            0b0000_0000_0000_0000_0000_0000_0111_1111,
        ),
        (
            0b1111_1111_1111_1111_1111_1111_1111_1111,
            0b1111_1111_1111_1111_1111_1111_1111_1111,
        ),
        (
            0b1111_1111_1111_1111_1111_1111_0000_0000,
            0b0000_0000_0000_0000_0000_0000_0000_0000,
        ),
    ]);

    for (input, expected_out) in cases {
        assert_eq!(((input as i8) as i32) as u32, expected_out, "invalid test case");

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

fn assert_single_output(expected: u64, outputs: Vec<u64>) {
    let rest_all_zero = outputs.iter().skip(1).all(|&x| x == 0);
    assert!(
        rest_all_zero,
        "expected all elements after first to be zero, but got {outputs:?}"
    );

    assert_eq!(outputs[0], expected);
}

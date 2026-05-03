use miden_core::Felt;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

/// Regression test for https://github.com/0xMiden/compiler/issues/872.
///
/// Previously, compilation could panic during stack manipulation with 16 live stack operands.
#[test]
fn test_invalid_stack_index_16_issue_872() {
    let main_fn = r#"
        (a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
         a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt) -> Felt {
            // Keep locals live across the call which are used only after the call, so that the 16
            // call arguments are not at the top of the operand stack at call time.
            let post = a0 + miden_stdlib_sys::felt!(1);

            let res = callee_16(a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);

            // Use all post-call locals to prevent DCE.
            res + post
        }

        #[inline(never)]
        fn callee_16(
            a0: Felt, a1: Felt, a2: Felt, a3: Felt, a4: Felt, a5: Felt, a6: Felt, a7: Felt,
            a8: Felt, a9: Felt, a10: Felt, a11: Felt, a12: Felt, a13: Felt, a14: Felt, a15: Felt,
        ) -> Felt {
            a0 + a1 + a2 + a3 + a4 + a5 + a6 + a7 + a8 + a9 + a10 + a11 + a12 + a13 + a14 + a15
        }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("movup_16_issue_831", main_fn, config, []);

    let package = test.compile_package();

    // This should execute and return the expected value.
    let args: [Felt; 16] = [
        Felt::from(1u32),
        Felt::from(2u32),
        Felt::from(3u32),
        Felt::from(4u32),
        Felt::from(5u32),
        Felt::from(6u32),
        Felt::from(7u32),
        Felt::from(8u32),
        Felt::from(9u32),
        Felt::from(10u32),
        Felt::from(11u32),
        Felt::from(12u32),
        Felt::from(13u32),
        Felt::from(14u32),
        Felt::from(15u32),
        Felt::from(16u32),
    ];

    let expected = (1u32..=16u32).fold(Felt::ZERO, |acc, x| acc + Felt::from(x)) + Felt::from(2u32);

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, expected);
        Ok(())
    })
    .unwrap();
}

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

#[test]
fn test_invalid_stack_index_4_word_1_felt_args() {
    let main_fn = r#"
        (
            w0_0: Felt,
            w0_1: Felt,
            w0_2: Felt,
            w0_3: Felt,
            w1_0: Felt,
            w1_1: Felt,
            w1_2: Felt,
            w1_3: Felt,
            w2_0: Felt,
            w2_1: Felt,
            w2_2: Felt,
            w2_3: Felt,
            w3_0: Felt,
            w3_1: Felt,
            w3_2: Felt,
            w3_3: Felt,
        ) -> Felt {
            let w0 = Word::new([w0_0, w0_1, w0_2, w0_3]);
            let w1 = Word::new([w1_0, w1_1, w1_2, w1_3]);
            let w2 = Word::new([w2_0, w2_1, w2_2, w2_3]);
            let w3 = Word::new([w3_0, w3_1, w3_2, w3_3]);

            // Keep locals live across the call which are used only after the call, so that the
            // call arguments are not at the top of the operand stack at call time.
            let post = w0[0] + miden_stdlib_sys::felt!(1);

            let extra = w0[1];
            let res = callee_5(w0, w1, w2, w3, extra);

            // Use all post-call locals to prevent DCE.
            res + post
        }

        #[inline(never)]
        fn callee_5(w0: Word, w1: Word, w2: Word, w3: Word, extra: Felt) -> Felt {
            w0[0] + w0[1] + w0[2] + w0[3] +
            w1[0] + w1[1] + w1[2] + w1[3] +
            w2[0] + w2[1] + w2[2] + w2[3] +
            w3[0] + w3[1] + w3[2] + w3[3] +
            extra
        }
    "#;

    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "movup_4_word_1_felt_args_invalid_stack_index",
        main_fn,
        config,
        [],
    );

    let package = test.compile_package();

    // This should execute and return the expected value.
    let args: [Felt; 16] = [
        // w0
        Felt::from(1u32),
        Felt::from(2u32),
        Felt::from(3u32),
        Felt::from(4u32),
        // w1
        Felt::from(5u32),
        Felt::from(6u32),
        Felt::from(7u32),
        Felt::from(8u32),
        // w2
        Felt::from(9u32),
        Felt::from(10u32),
        Felt::from(11u32),
        Felt::from(12u32),
        // w3
        Felt::from(13u32),
        Felt::from(14u32),
        Felt::from(15u32),
        Felt::from(16u32),
    ];

    // Expected:
    // - callee_5 sums 1..=16 and adds `extra` (w0[1] == 2)
    // - main adds `post` (w0[0] + 1 == 2)
    let expected = (1u32..=16u32).fold(Felt::ZERO, |acc, x| acc + Felt::from(x)) + Felt::from(4u32);

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, expected);
        Ok(())
    })
    .unwrap();
}

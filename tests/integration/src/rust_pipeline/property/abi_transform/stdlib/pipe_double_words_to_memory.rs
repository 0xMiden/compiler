use core::panic;
use std::collections::VecDeque;

use miden_core::{Word, advice::AdviceStackBuilder, utils::group_slice_elements};
use miden_debug::{Executor, ToMidenRepr};
use miden_processor::advice::AdviceInputs;
use midenc_expect_test::expect_file;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use midenc_session::Emit;
use proptest::{
    arbitrary::any,
    prelude::TestCaseError,
    prop_assert_eq,
    test_runner::{TestError, TestRunner},
};

use crate::{
    CompilerTest,
    testing::{Initializer, eval_package, eval_package_with_advice_stack},
};
#[test]
fn test_pipe_double_words_to_memory() {
    let main_fn = r#"
        (h0: Felt, h1: Felt, h2: Felt, h3: Felt, num_words: Felt) -> Felt {
            let expected = Word::new([h0, h1, h2, h3]);
            let (state_word, copied) = miden_stdlib_sys::pipe_double_words_to_memory(num_words);
            assert_eq!(state_word, expected);
            let mut acc = felt!(0);
            for v in copied {
                acc = acc + v;
            }
            acc
        }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "pipe_double_words_to_memory",
        &main_fn,
        config,
        ["--test-harness".into()],
    );

    let package = test.compile_package();

    let config = proptest::test_runner::Config::with_cases(32);
    let res =
        TestRunner::new(config).run(&any::<Vec<[miden_debug::Felt; 4]>>(), move |test_words| {
            let mut raw_words: Vec<[Felt; 4]> = test_words
                .into_iter()
                .map(|w| [w[0].into(), w[1].into(), w[2].into(), w[3].into()])
                .collect();

            // `pipe_double_words_to_memory` requires an even number of words.
            if !raw_words.len().is_multiple_of(2) {
                raw_words.push([Felt::ZERO; 4]);
            }

            let mut flat_felts: Vec<Felt> = Vec::with_capacity(raw_words.len() * 4);
            for w in &raw_words {
                flat_felts.extend_from_slice(w);
            }
            let expected_sum = flat_felts.iter().copied().fold(Felt::ZERO, |acc, v| acc + v);
            let expected_digest = miden_core::crypto::hash::Poseidon2::hash_elements(&flat_felts);

            let mut advice_builder = AdviceStackBuilder::new();
            advice_builder.push_for_adv_pipe(&flat_felts);
            let advice_stack = advice_builder.into_elements();

            let args = [
                expected_digest[0],
                expected_digest[1],
                expected_digest[2],
                expected_digest[3],
                Felt::from(raw_words.len() as u32),
            ];

            eval_package_with_advice_stack::<Felt, _, _, _>(
                &package,
                [],
                advice_stack,
                &args,
                &test.session,
                |trace| {
                    let res: Felt = trace.parse_result().unwrap();
                    prop_assert_eq!(res, expected_sum);
                    Ok(())
                },
            )?;

            Ok(())
        });

    match res {
        Err(TestError::Fail(_, value)) => {
            panic!("Found minimal(shrinked) failing case: {:?}", value);
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

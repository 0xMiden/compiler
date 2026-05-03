use core::panic;

use miden_core::{Word, advice::AdviceStackBuilder};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{
    arbitrary::any,
    prop_assert_eq,
    test_runner::{TestError, TestRunner},
};

use crate::{CompilerTest, testing::eval_package_with_advice_stack};

#[test]
fn test_pipe_words_to_memory() {
    let main_fn = r#"
        (h0: Felt, h1: Felt, h2: Felt, h3: Felt, num_words: Felt) -> Felt {
            let expected = Word::new([h0, h1, h2, h3]);
            let (state_word, copied) = miden_stdlib_sys::pipe_words_to_memory(num_words);
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
        "pipe_words_to_memory",
        &main_fn,
        config,
        ["--test-harness".into()],
    );

    let package = test.compile_package();

    let config = proptest::test_runner::Config::with_cases(32);
    let res =
        TestRunner::new(config).run(&any::<Vec<[miden_debug::Felt; 4]>>(), move |test_words| {
            let raw_words: Vec<[Felt; 4]> = test_words
                .into_iter()
                .map(|w| [w[0].into(), w[1].into(), w[2].into(), w[3].into()])
                .collect();

            let mut flat_felts: Vec<Felt> = Vec::with_capacity(raw_words.len() * 4);
            for w in &raw_words {
                flat_felts.extend_from_slice(w);
            }
            let expected_sum = flat_felts.iter().copied().fold(Felt::ZERO, |acc, v| acc + v);
            let expected_digest = miden_core::crypto::hash::Poseidon2::hash_elements(&flat_felts);

            let mut advice_builder = AdviceStackBuilder::new();

            // `pipe_words_to_memory` consumes words via `adv_pipe` in pairs, then (if needed)
            // consumes a final word via `adv_loadw`.
            let has_odd_word = (raw_words.len() % 2) == 1;
            let pairs_len_words = if has_odd_word {
                raw_words.len() - 1
            } else {
                raw_words.len()
            };

            if pairs_len_words > 0 {
                let mut pipe_elems = Vec::with_capacity(pairs_len_words * 4);
                for w in raw_words.iter().take(pairs_len_words) {
                    pipe_elems.extend_from_slice(w);
                }
                advice_builder.push_for_adv_pipe(&pipe_elems);
            }

            if has_odd_word {
                let last = raw_words.last().expect("raw_words is non-empty when has_odd_word");
                advice_builder.push_for_adv_loadw(Word::new(*last));
            }

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

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
fn test_hash_words() {
    // Similar to test_hash_elements, but passes Vec<Word> and uses hash_words
    let main_fn = r#"
	    (input: alloc::vec::Vec<miden_stdlib_sys::Word>) -> miden_stdlib_sys::Felt {
	        let res = miden_stdlib_sys::hash_words(&input);
	        // Return the first limb of the digest for easy comparison
	        res.inner[0]
	    }"#
    .to_string();

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "hash_words",
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

            let expected_digest = miden_core::crypto::hash::Poseidon2::hash_elements(&flat_felts);

            let wide_ptr_addr = 20u32 * 65536;

            let mut wide_ptr: Vec<Felt> = vec![
                Felt::from(raw_words.capacity() as u32),
                Felt::from(wide_ptr_addr + 16), // pointer to first element just past header
                Felt::from(raw_words.len() as u32),
                Felt::ZERO,
            ];
            for w in &raw_words {
                wide_ptr.extend_from_slice(w);
            }

            let initializers = [Initializer::MemoryFelts {
                addr: wide_ptr_addr / 4,
                felts: (&wide_ptr).into(),
            }];

            let args = [Felt::new(wide_ptr_addr as u64)];

            eval_package::<Felt, _, _>(&package, initializers, &args, &test.session, |trace| {
                let res: Felt = trace.parse_result().unwrap();
                prop_assert_eq!(res, expected_digest[0]);
                Ok(())
            })?;

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

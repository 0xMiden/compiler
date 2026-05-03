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
fn test_hash_elements() {
    let main_fn = r#"
	    (input: alloc::vec::Vec<miden_stdlib_sys::Felt>) -> miden_stdlib_sys::Felt {
	        let res = miden_stdlib_sys::hash_elements(input);
	        res.inner[0]
	    }"#
    .to_string();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(
        "hash_elements",
        &main_fn,
        config,
        ["--test-harness".into()],
    );

    let package = test.compile_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    let config = proptest::test_runner::Config::with_cases(32);
    let res = TestRunner::new(config).run(&any::<Vec<miden_debug::Felt>>(), move |test_felts| {
        let raw_felts: Vec<Felt> = test_felts.into_iter().map(From::from).collect();

        let expected_digest = miden_core::crypto::hash::Poseidon2::hash_elements(&raw_felts);
        let wide_ptr_addr = 20u32 * 65536; // 1310720

        // The order below is exactly the order Rust compiled code is expected to have the data
        // layed out in the fat pointer for the entrypoint.
        let mut wide_ptr = vec![
            Felt::from(raw_felts.capacity() as u32),
            Felt::from(wide_ptr_addr + 16),
            Felt::from(raw_felts.len() as u32),
            Felt::ZERO,
        ];
        wide_ptr.extend_from_slice(&raw_felts);
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

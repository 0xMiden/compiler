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
fn test_vec_alloc_vec() {
    // regression test for https://github.com/0xMiden/compiler/issues/595
    let main_fn = r#"
    (a: u32) -> Felt {
        let input: alloc::vec::Vec<Felt> = alloc::vec![
            felt!(1),
            felt!(2),
            felt!(3),
        ];
        input[a as usize]
    }
    "#
    .to_string();
    let config = WasmTranslationConfig::default();
    let mut test =
        CompilerTest::rust_fn_body_with_stdlib_sys("vec_alloc_vec", &main_fn, config, []);

    let package = test.compile_package();

    let args = [Felt::from(2u32)];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: u32 = trace.parse_result().unwrap();
        assert_eq!(res, 3, "unexpected result (regression test for https://github.com/0xMiden/compiler/issues/595)");
        Ok(())
    })
    .unwrap();
}

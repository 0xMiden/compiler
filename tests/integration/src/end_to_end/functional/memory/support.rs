use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{
    CompilerTest,
    testing::{eval_package, setup},
};

pub(super) fn assert_memory_test_returns_zero(artifact_name: &'static str, main_fn: &str) {
    setup::enable_compiler_instrumentation();
    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_fn_body_with_stdlib_sys(artifact_name, main_fn, config, []);

    let package = test.compile_package();
    let args: [Felt; 0] = [];

    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let res: Felt = trace.parse_result().unwrap();
        assert_eq!(res, Felt::ZERO);
        Ok(())
    })
    .unwrap();
}

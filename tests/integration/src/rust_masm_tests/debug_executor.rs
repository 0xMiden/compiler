use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::eval_package};

#[test]
fn test_println() {
    let main_fn = "() -> u32 { println(\"hello\"); 0 }";
    let mut test = CompilerTest::rust_fn_body_with_sdk(
        "test_println",
        main_fn,
        WasmTranslationConfig::default(),
        [],
    );

    let package = test.compile_package();

    let args = [];
    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: Felt = trace.parse_result().unwrap();
        assert_eq!(result, Felt::from(0u32));
        assert_eq!(trace.printed_lines().len(), 1);
        assert_eq!(trace.printed_lines().values().next().unwrap(), "hello");
        Ok(())
    })
    .unwrap();
}

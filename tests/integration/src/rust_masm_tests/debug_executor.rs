use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::eval_package};

#[test]
fn test_println_static() {
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

// Manipulates bytes to avoid `call_indirect`, which gets triggered by Rust's formatting infra.
#[test]
fn test_println_dynamic() {
    let main_fn = r#"(digit: u32) -> u32 {
        let mut s = alloc::string::String::from("the digit is: ");
        // b'0' is zero as ASCII, then add `digit` as offset
        s.push((b'0' + (digit as u8)) as char);
        println(&s);
        0
    }"#;
    let mut test = CompilerTest::rust_fn_body_with_sdk(
        "test_println_dynamic",
        main_fn,
        WasmTranslationConfig::default(),
        [],
    );

    let package = test.compile_package();

    let args = [Felt::from(7u32)];
    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: Felt = trace.parse_result().unwrap();
        assert_eq!(result, Felt::from(0u32));
        assert_eq!(trace.printed_lines().len(), 1);
        assert_eq!(trace.printed_lines().values().next().unwrap(), "the digit is: 7");
        Ok(())
    })
    .unwrap();
}

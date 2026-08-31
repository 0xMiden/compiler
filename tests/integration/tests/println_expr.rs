mod common;

use common::info_messages_since;
use miden_core::Felt;
use miden_debug::logger::DebugLogger;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_tests::{CompilerTest, testing::eval_package};

/// Exercises the expression arm of the `println` macro.
#[test]
fn println_expr() {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    log::set_max_level(log::LevelFilter::Warn);

    let main_fn = r#"(digit: u32) -> u32 {
        let mut s = ::alloc::string::String::from("the digit is: ");
        // b'0' is zero as ASCII, then add `digit` as offset
        s.push((b'0' + (digit as u8)) as char);
        println!(s.as_str());

        // Use string formatting in the expression.
        println!(::alloc::format!("{digit} is the digit").as_str());

        0
    }"#;
    let mut test = CompilerTest::rust_fn_body_with_sdk(
        "test_println_dynamic",
        main_fn,
        WasmTranslationConfig::default(),
        [],
    );

    let package = test.compile_package();
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    let args = [Felt::from(7u32)];
    eval_package::<Felt, _, _>(package, [], &args, &test.session, |trace| {
        let result: Felt = trace.parse_result().unwrap();
        assert_eq!(result, Felt::from(0u32));
        Ok(())
    })
    .unwrap();

    let info_messages = info_messages_since(before);
    assert_eq!(
        info_messages.as_slice(),
        ["the digit is: 7", "7 is the digit"],
        "observed info messages: {info_messages:?}",
    );
}

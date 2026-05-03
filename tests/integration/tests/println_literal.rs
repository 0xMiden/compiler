use log::Level;
use miden_core::Felt;
use miden_debug::logger::DebugLogger;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_tests::{CompilerTest, testing::eval_package};

#[test]
fn test_println_static() {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    log::set_max_level(log::LevelFilter::Warn);

    let main_fn = "() -> u32 { println!(\"hello\"); 0 }";
    let mut test = CompilerTest::rust_fn_body_with_sdk(
        "test_println",
        main_fn,
        WasmTranslationConfig::default(),
        [],
    );

    let package = test.compile_package();
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    let args = [];
    eval_package::<Felt, _, _>(&package, [], &args, &test.session, |trace| {
        let result: Felt = trace.parse_result().unwrap();
        assert_eq!(result, Felt::from(0u32));
        Ok(())
    })
    .unwrap();

    let logs: Vec<_> = DebugLogger::get().clone_captured().into_iter().skip(before).collect();
    let info_messages: Vec<_> = logs
        .iter()
        .filter(|entry| entry.level == Level::Info)
        .map(|entry| entry.message.as_str())
        .collect();
    assert_eq!(
        info_messages.as_slice(),
        ["hello"],
        "observed logs: {:?}",
        logs.iter().map(|e| format!("{}: {}", e.level, e.message)).collect::<Vec<_>>(),
    );
}

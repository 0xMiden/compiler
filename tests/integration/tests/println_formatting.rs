use log::Level;
use miden_core::Felt;
use miden_debug::logger::DebugLogger;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_tests::{CompilerTest, testing::eval_package};

/// Exercises the formatting arm of the `println` macro.
#[test]
fn println_formatting() {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    log::set_max_level(log::LevelFilter::Warn);

    let main_fn = r#"(a: u32, b: u32) -> u32 {
        println!("can format: {}", true);
        println!("the inputs are: {}, {}", a, b);
        println!("function call: {}", core::convert::identity(a));
        println!("reference: {}", &b);
        println!("expression: {}", a + b);
        println!("mixed: {}, {}", core::convert::identity(a), a + b);
        match a {
            7 => println!("match arm: {}", a),
            _ => println!("match arm: {}", b),
        }

        // Examples from https://doc.rust-lang.org/std/fmt/index.html#usage
        println!("Hello");
        println!("Hello, {}!", "world");
        println!("The number is {}", 1);
        println!("{:?}", (3, 4));
        println!("{value}", value = 4);
        let people = "Rustaceans";
        println!("Hello {people}!", people = people);
        println!("{} {}", 1, 2);
        println!("{:04}", 42);
        println!("{:#?}", (100, 200));

        0
    }"#;
    let mut test = CompilerTest::rust_fn_body_with_sdk(
        "test_println_formatting",
        main_fn,
        WasmTranslationConfig::default(),
        [],
    );

    let package = test.compile_package();
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    let args = [Felt::from(7u32), Felt::from(42u32)];
    eval_package::<Felt, _, _>(package, [], &args, &test.session, |trace| {
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
        [
            "can format: true",
            "the inputs are: 7, 42",
            "function call: 7",
            "reference: 42",
            "expression: 49",
            "mixed: 7, 49",
            "match arm: 7",
            "Hello",
            "Hello, world!",
            "The number is 1",
            "(3, 4)",
            "4",
            "Hello Rustaceans!",
            "1 2",
            "0042",
            "(\n    100,\n    200,\n)",
        ],
        "observed logs: {:?}",
        logs.iter().map(|e| format!("{}: {}", e.level, e.message)).collect::<Vec<_>>(),
    );
}

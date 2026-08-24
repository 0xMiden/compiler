//! Tests that a custom `#[panic_handler]` is invoked when the fixture is compiled with
//! `-Cpanic=abort`.
//!
//! Handler invocation is confirmed by observing `PANIC_HANDLER_MESSAGE`.
mod common;

use common::{PANIC_HANDLER_MESSAGE, PanicStrategy, info_messages_since};
use miden_core::Felt;
use miden_debug::logger::DebugLogger;
use midenc_integration_tests::testing::executor_with_std;

#[test]
fn panic_handler_invoked_when_compiled_with_abort() {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    log::set_max_level(log::LevelFilter::Warn);

    let mut test = common::compile_panic_handler_fixture(PanicStrategy::Abort);
    let package = test.compile_package();
    common::print_pkg_size_for_panic_strategy(PanicStrategy::Abort, &package);
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    // On non-panicking path, the function returns its input, and the handler is silent
    {
        let exec = executor_with_std(vec![Felt::from(200u32)]);
        let trace = exec.execute(package.clone(), test.session.source_manager.clone());
        let result: u32 = trace.parse_result().expect("failed to parse result");
        assert_eq!(result, 200, "when x > 100, function should return x");
        let messages = info_messages_since(before);
        assert!(
            messages.iter().all(|msg| !msg.contains(PANIC_HANDLER_MESSAGE)),
            "panic handler must not run on the non-panicking path, recorded messages: {messages:?}"
        );
    }

    // On the panicking path: execution traps, but only after the handler printed its message
    let trap_message = common::execute_expecting_trap(&test, package, vec![Felt::from(50u32)]);
    let messages = info_messages_since(before);
    assert!(
        messages.iter().any(|msg| msg.contains(PANIC_HANDLER_MESSAGE)),
        "expected the custom panic handler to print '{PANIC_HANDLER_MESSAGE}', recorded messages: \
         {messages:?}, trap: {trap_message}"
    );
}

//! Tests that a custom `#[panic_handler]` is *not* invoked under the `-Cpanic=immediate-abort`
//! strategy.
//!
//! TODO use panic infra once its added
//! Handler invocation would be observable through the `PrintLn` event the handler emits.

mod common;

use common::PANIC_HANDLER_MESSAGE;
use miden_core::Felt;
use miden_debug::logger::DebugLogger;

#[test]
fn panic_handler_not_invoked_with_default_immediate_abort() {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    log::set_max_level(log::LevelFilter::Warn);

    let mut test = common::compile_panic_handler_fixture(common::PanicStrategy::ImmediateAbort);
    let package = test.compile_package();
    common::print_pkg_size_for_panic_strategy(common::PanicStrategy::ImmediateAbort, &package);
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    let trap_message = common::execute_expecting_trap(&test, package, vec![Felt::from(50u32)]);

    // The handler's message must not appear at any `Info`-level output.
    let messages = common::info_messages_since(before);
    assert!(
        messages.iter().all(|msg| !msg.contains(PANIC_HANDLER_MESSAGE)),
        "expected the custom panic handler to not run under immediate-abort, observed messages: \
         {messages:?}, trap: {trap_message}",
    );
}

//! Helpers used only by the process-isolated panic-handler tests.

use std::panic::{self, AssertUnwindSafe};

use miden_core::Felt;
use miden_debug::logger::DebugLogger;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_tests::{CompilerTest, testing::executor_with_std};
use midenc_session::PanicStrategy;

use super::info_messages_since;

/// The message emitted only by the fixture's custom panic handler.
pub const PANIC_HANDLER_MESSAGE: &str = "custom panic handler invoked";
/// The message supplied to the fixture's assertion.
pub const ASSERTION_MESSAGE: &str = "input smaller than threshold";

/// Compiles and executes the given panic-handler fixture, returning its captured info messages.
pub fn panic_handler_messages_for_fixture(
    fixture: &str,
    entrypoint: &str,
    strategy: PanicStrategy,
) -> Vec<String> {
    DebugLogger::init_for_tests()
        .expect("each test using DebugLogger should run in its own process");
    // Keep compiler info logs out of the test output, then enable them only for execution.
    log::set_max_level(log::LevelFilter::Warn);

    let mut test = CompilerTest::rust_source_cargo_miden(
        fixture,
        WasmTranslationConfig::default(),
        [
            "--entrypoint".to_string(),
            entrypoint.to_string(),
            strategy.as_rustc_arg().to_string(),
        ],
    );
    let package = test.compile_package();
    let before = DebugLogger::get().clone_captured().len();
    log::set_max_level(log::LevelFilter::Info);

    let executor = executor_with_std(vec![Felt::from(50u32)]);
    let source_manager = test.session.source_manager.clone();
    let result =
        panic::catch_unwind(AssertUnwindSafe(move || executor.execute(package, source_manager)));
    let payload = match result {
        Ok(_) => panic!("expected the fixture's assertion to trap"),
        Err(payload) => payload,
    };
    let message = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .expect("execution failure should have a string payload");
    assert!(
        message.contains("assertion failed with error message: entered unreachable code"),
        "expected an unreachable trap, got: {message}"
    );

    info_messages_since(before)
}

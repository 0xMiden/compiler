//! Shared helpers for the process-isolated integration tests.

// This module is compiled into each integration test binary separately, and each
// binary only uses a subset of these helpers, so allow unused items.
#![allow(dead_code)]

use std::{
    panic::{self, AssertUnwindSafe},
    sync::Arc,
};

use log::Level;
use miden_core::{Felt, serde::Serializable};
use miden_debug::logger::DebugLogger;
use miden_mast_package::Package;
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_integration_tests::{
    CompilerTest, CompilerTestBuilder,
    testing::{executor_with_std, stripped_mast_size_str},
};
pub use midenc_session::PanicStrategy;

const PANIC_HANDLER_FIXTURE: &str = "../fixtures/components/panic-handler-test";

/// The message the fixture's custom `#[panic_handler]` prints when invoked.
pub const PANIC_HANDLER_MESSAGE: &str = "custom panic handler invoked";

/// Compile the `panic-handler-test` fixture with the given panic strategy.
pub fn compile_panic_handler_fixture(strategy: PanicStrategy) -> CompilerTest {
    CompilerTestBuilder::rust_source_cargo_miden(
        PANIC_HANDLER_FIXTURE,
        WasmTranslationConfig::default(),
        [
            "--entrypoint".to_string(),
            "panic_handler_test::entrypoint".to_string(),
            "-C".to_string(),
            format!("panic={}", strategy.as_str()),
        ],
    )
    .build()
}

/// The `Info`-level messages captured since `before` (a snapshot taken after compilation).
pub fn info_messages_since(before: usize) -> Vec<String> {
    DebugLogger::get()
        .clone_captured()
        .into_iter()
        .skip(before)
        .filter(|entry| entry.level == Level::Info)
        .map(|entry| entry.message)
        .collect()
}

/// Execute the provided package and assert that execution trapped.
///
/// Returns the panic message produced by the executor for the trapped execution.
pub fn execute_expecting_trap(
    test: &CompilerTest,
    package: Arc<Package>,
    input_stack: Vec<Felt>,
) -> String {
    let input_stack_debug = format!("{input_stack:?}");
    let source_manager = test.session.source_manager.clone();
    let exec = executor_with_std(input_stack);
    let result =
        panic::catch_unwind(AssertUnwindSafe(move || exec.execute(package, source_manager)));
    match result {
        Ok(_) => {
            panic!("expected execution to trap but it did not, input_stack = {input_stack_debug}")
        }
        Err(payload) => {
            if let Some(s) = payload.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = payload.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "unknown panic".to_string()
            }
        }
    }
}

pub fn print_pkg_size_for_panic_strategy(strategy: PanicStrategy, package: &Package) {
    let full = package.to_bytes().len();
    let mast = stripped_mast_size_str(package);
    eprintln!(
        "[code size] panic={}: package = {full} bytes, mast forest = {mast} bytes",
        strategy.as_str()
    );
}

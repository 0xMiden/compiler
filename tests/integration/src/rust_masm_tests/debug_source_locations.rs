//! Tests that verify debug source location information is correctly preserved
//! from Rust source code through to MASM compilation and execution.

use std::panic::{self, AssertUnwindSafe};

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::executor_with_std};

#[test]
fn test_rust_assert_macro_source_location_with_debug_executor() {
    let config = WasmTranslationConfig::default();

    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/assert-debug-test",
        config,
        [],
    );

    let package = test.compile_package();
    let program = package.unwrap_program();

    // First, test that the function works when assertion passes (x > 100)
    {
        let args = vec![Felt::new(200)];
        let exec = executor_with_std(args, Some(&package));

        let trace = exec.execute(&program, test.session.source_manager.clone());
        let result: u32 = trace.parse_result().expect("Failed to parse result");
        assert_eq!(result, 200, "When x > 100, function should return x");
    }

    // Now test that when assertion fails (x <= 100), we get a panic with source location
    {
        let args = vec![Felt::new(50)];
        let exec = executor_with_std(args, Some(&package));

        let program_clone = program.clone();
        let source_manager = test.session.source_manager.clone();

        let result = panic::catch_unwind(AssertUnwindSafe(move || {
            exec.execute(&program_clone, source_manager)
        }));

        let panic_message = match result {
            Ok(_) => panic!("Expected execution to fail due to assertion (x=50 <= 100)"),
            Err(panic_info) => {
                if let Some(s) = panic_info.downcast_ref::<String>() {
                    s.clone()
                } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                    s.to_string()
                } else {
                    "Unknown panic".to_string()
                }
            }
        };

        if !panic_message.contains("lib.rs") || !panic_message.contains(":26:13") {
            dbg!(&panic_message);
            panic!("Panic message should contain source location 'lib.rs' and ':26:13'");
        }
    }
}

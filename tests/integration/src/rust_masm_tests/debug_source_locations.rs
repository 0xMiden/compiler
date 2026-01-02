//! Tests that verify debug source location information is correctly preserved
//! from Rust source code through to MASM compilation and execution.

use std::{
    panic::{self, AssertUnwindSafe},
    path::PathBuf,
};

use miden_core::Felt;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::executor_with_std};

/// Get the absolute path to the assert-debug-test fixture.
fn get_test_fixture_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| std::env::current_dir().unwrap().to_str().unwrap().to_string());
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .join("rust-apps-wasm")
        .join("rust-sdk")
        .join("assert-debug-test")
}

#[test]
fn test_rust_assert_macro_source_location_with_debug_executor() {
    let config = WasmTranslationConfig::default();
    let fixture_path = get_test_fixture_path();
    let fixture_path_str = fixture_path.to_string_lossy();

    // Note: cargo-miden automatically:
    // 1. Passes --debug to midenc
    // 2. Adds -Ztrim-path-prefix when debug is enabled
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/assert-debug-test",
        config,
        [],
    );

    eprintln!("\n=== Fixture path ===");
    eprintln!("{fixture_path_str}");
    eprintln!("============================================\n");

    let package = test.compiled_package();
    let program = package.unwrap_program();

    // First, test that the function works when assertion passes (x > 100)
    {
        let args = vec![Felt::new(200)];
        let exec = executor_with_std(args, Some(&package));

        let trace = exec.execute(&program, test.session.source_manager.clone());
        let result: u32 = trace.parse_result().expect("Failed to parse result");
        assert_eq!(result, 200, "When x > 100, function should return x");
        eprintln!("SUCCESS: Assertion passed when x=200 > 100");
    }

    // Now test that when assertion fails (x <= 100), we get a panic with source location
    {
        let args = vec![Felt::new(50)]; // x = 50, assert!(50 > 100) fails
        let exec = executor_with_std(args, Some(&package));

        // Clone values needed for the closure
        let program_clone = program.clone();
        let source_manager = test.session.source_manager.clone();

        // Capture the panic output
        let result = panic::catch_unwind(AssertUnwindSafe(move || {
            exec.execute(&program_clone, source_manager)
        }));

        // The execution should panic (fail) because assert!(50 > 100) fails
        assert!(
            result.is_err(),
            "Execution should have panicked due to failed assertion (x=50 <= 100)"
        );

        // Check the panic message for source location information
        if let Err(panic_info) = result {
            let panic_message = if let Some(s) = panic_info.downcast_ref::<String>() {
                s.clone()
            } else if let Some(s) = panic_info.downcast_ref::<&str>() {
                s.to_string()
            } else {
                "Unknown panic".to_string()
            };

            eprintln!("\n=== Panic message from failed assertion ===");
            eprintln!("{panic_message}");
            eprintln!("============================================\n");

            // The panic message should indicate an assertion failure
            assert!(
                panic_message.contains("assertion failed"),
                "Panic message should indicate assertion failure. Got: {panic_message}"
            );

            // Check if source location info is present - the assert! macro is on line 26
            let has_source_file = panic_message.contains("assert-debug-test/src/lib.rs");
            let has_lib_rs = panic_message.contains("lib.rs");

            // Line 26 contains: assert!(x > 100);
            let has_line_info = panic_message.contains(":26:");

            // Column 13 is where the assertion expression starts
            let has_column_info = panic_message.contains(":26:13");

            eprintln!("SUCCESS: Assertion correctly failed when x=50 <= 100");
            eprintln!(
                "Has source file reference (assert-debug-test/src/lib.rs): {has_source_file}"
            );
            eprintln!("Has lib.rs reference: {has_lib_rs}");
            eprintln!("Has line info (:26:): {has_line_info}");
            eprintln!("Has column info (:26:13): {has_column_info}");

            // Source locations MUST be resolved now - this is a regression test
            assert!(
                has_lib_rs,
                "Panic message should contain source file reference (lib.rs). Got: {panic_message}"
            );
            assert!(
                has_line_info,
                "Panic message should contain line 26 (where assert! is). Got: {panic_message}"
            );
            assert!(
                has_column_info,
                "Panic message should contain column 13 (:26:13). Got: {panic_message}"
            );

            eprintln!("Source locations are being resolved correctly!");
        }
    }
}

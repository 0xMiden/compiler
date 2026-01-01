//! Tests that verify debug source location information is correctly preserved
//! from Rust source code through to MASM compilation and execution.

use std::panic::{self, AssertUnwindSafe};

use miden_core::Felt;
use midenc_expect_test::expect;
use midenc_frontend_wasm::WasmTranslationConfig;

use crate::{CompilerTest, testing::executor_with_std};

#[test]
fn test_rust_assert_macro_source_location_with_debug_executor() {
    let config = WasmTranslationConfig::default();

    // Note: cargo-miden automatically:
    // 1. Passes --debug to midenc
    // 2. Adds -Ztrim-path-prefix when debug is enabled
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../rust-apps-wasm/rust-sdk/assert-debug-test",
        config,
        [],
    );

    let package = test.compiled_package();
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
        // Extract and validate the panic message with source location
        let panic_message = match result {
            Ok(_) => panic!("Expected execution to fail with assertion error (x=50 <= 100)"),
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

        // The panic message should contain the source location.
        // Extract just the source location ("assert-debug-test/src/lib.rs:26:13")
        let panic_message = strip_ansi_codes(&panic_message);
        let source_location = extract_source_location(&panic_message);
        expect![[r#"assert-debug-test/src/lib.rs:26:13"#]].assert_eq(&source_location);
    }
}

/// Extract the source location from a panic message.
fn extract_source_location(s: &str) -> String {
    // Look for the pattern: assert-debug-test/src/lib.rs:LINE:COL
    if let Some(start) = s.find("assert-debug-test/") {
        let rest = &s[start..];
        // Location ends at ']' or whitespace or end of string
        let end = rest.find([']', '\n', ' ']).unwrap_or(rest.len());
        return rest[..end].to_string();
    }
    "source location not found".to_string()
}

/// Strip ANSI escape codes from a string.
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next == 'm' {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

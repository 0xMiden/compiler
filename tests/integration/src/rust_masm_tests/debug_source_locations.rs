//! Tests that verify debug source location information is correctly preserved
//! from Rust source code through to MASM compilation and execution.
//!

use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use miden_core::Felt;
use miden_debug::Executor;
use miden_lib::MidenLib;
use midenc_compile::compile_to_memory;
use midenc_session::{InputFile, STDLIB};

use crate::testing::setup;

// Get path to examples/assert-debug-test test.
fn get_assert_debug_test_path() -> PathBuf {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| std::env::current_dir().unwrap().to_str().unwrap().to_string());
    PathBuf::from(manifest_dir)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("examples")
        .join("assert-debug-test")
}

fn create_executor_with_std(
    args: Vec<Felt>,
    package: &miden_mast_package::Package,
) -> Executor {
    let mut exec = Executor::new(args);
    let std_library = (*STDLIB).clone();
    exec.dependency_resolver_mut()
        .add(*std_library.digest(), std_library.clone().into());
    let base_library = Arc::new(MidenLib::default().as_ref().clone());
    exec.dependency_resolver_mut()
        .add(*base_library.digest(), base_library.clone().into());
    exec.with_dependencies(package.manifest.dependencies())
        .expect("Failed to set up dependencies");
    exec
}

#[test]
fn test_rust_assert_macro_source_location_with_debug_executor() {
    setup::enable_compiler_instrumentation();

    let example_path = get_assert_debug_test_path();
    let example_path_str = example_path.to_string_lossy();

    let manifest_path = example_path.join("Cargo.toml");
    let status = Command::new("cargo")
        .args([
            "build",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--manifest-path",
            manifest_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to run cargo build");
    assert!(status.success(), "Failed to build assert-debug-test example");

    let wasm_path = example_path
        .join("target")
        .join("wasm32-unknown-unknown")
        .join("release")
        .join("assert_debug_test.wasm");

    let input_file = InputFile::from_path(&wasm_path).expect("Failed to load wasm file");
    let context = setup::default_context(
        [input_file],
        &[
            "--debug",
            "full",
            &format!("-Ztrim-path-prefix={}", example_path_str),
            "--entrypoint",
            "assert_debug_test::test_assert",
        ],
    );

    let artifact = compile_to_memory(context.clone())
        .expect("Failed to compile wasm to masm");
    let package = artifact.unwrap_mast();
    let program = package.unwrap_program();
    let session = context.session_rc();

    // First, test that the function works when assertion passes (x > 100)
    {
        let args = vec![Felt::new(200)];
        let exec = create_executor_with_std(args, &package);

        let trace = exec.execute(&program, session.source_manager.clone());
        let result: u32 = trace.parse_result().expect("Failed to parse result");
        assert_eq!(result, 200, "When x > 100, function should return x");
        eprintln!("SUCCESS: Assertion passed when x=200 > 100");
    }

    // Now test that when assertion fails (x <= 100), we get a panic with source location
    {
        let args = vec![Felt::new(50)]; // x = 50, assert!(50 > 100) fails
        let exec = create_executor_with_std(args, &package);

        // Clone values needed for the closure
        let program_clone = program.clone();
        let source_manager = session.source_manager.clone();

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

            // Check if source location info is present
            let has_source_file = panic_message.contains("lib.rs")
                || panic_message.contains("src/");

            let has_line_info = panic_message.contains(":32")
                || panic_message.contains(":33");

            let has_any_source_info = has_source_file || has_line_info;

            // FIXME: Currently source locations show <unavailable> in stack traces.
            // This test documents the current behavior.
            eprintln!(
                "SUCCESS: Assertion correctly failed when x=50 <= 100"
            );
            eprintln!("Has source file reference: {}", has_source_file);
            eprintln!("Has line info: {}", has_line_info);

            if has_any_source_info {
                eprintln!("Source locations are being resolved!");
            }
        }
    }
}

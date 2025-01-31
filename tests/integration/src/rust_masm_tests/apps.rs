use std::collections::VecDeque;

use expect_test::expect_file;
use midenc_debug::{Executor, PopFromStack, PushToStack};
use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use proptest::{prelude::*, test_runner::TestRunner};

use crate::{cargo_proj::project, CompilerTest, CompilerTestBuilder};

#[test]
fn fib() {
    let mut test =
        CompilerTest::rust_source_cargo("fib", "miden_integration_tests_rust_fib_wasm", "fib");
    // Test expected compilation artifacts
    test.expect_wasm(expect_file!["../../expected/fib.wat"]);
    test.expect_ir(expect_file!["../../expected/fib.hir"]);
    test.expect_masm(expect_file!["../../expected/fib.masm"]);
    // let ir_masm = test.ir_masm_program();
    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::default()
        .run(&(1u32..30), move |a| {
            let rust_out = miden_integration_tests_rust_fib::fib(a);
            let mut args = Vec::<Felt>::default();
            PushToStack::try_push(&a, &mut args);

            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            // args.reverse();
            // let emul_out: u32 =
            //     execute_emulator(ir_masm.clone(), &args).first().unwrap().clone().into();
            // prop_assert_eq!(rust_out, emul_out);
            Ok(())
        })
        .unwrap();
}

#[test]
fn fib_hir2() {
    let mut test = CompilerTest::rust_source_cargo("fib", "fib_hir2", "fib");
    let artifact_name = test.artifact_name().to_string();
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir2(expect_file![format!("../../expected/{artifact_name}.hir")]);

    /*
        test.expect_masm(expect_file!["../../expected/fib.masm"]);
        // let ir_masm = test.ir_masm_program();
        let package = test.compiled_package();

        // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
        TestRunner::default()
            .run(&(1u32..30), move |a| {
                let rust_out = miden_integration_tests_rust_fib::fib(a);
                let mut args = Vec::<Felt>::default();
                PushToStack::try_push(&a, &mut args);

                let exec = Executor::for_package(&package, args, &test.session)
                    .map_err(|err| TestCaseError::fail(err.to_string()))?;
                let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
                dbg!(output);
                prop_assert_eq!(rust_out, output);
                // args.reverse();
                // let emul_out: u32 =
                //     execute_emulator(ir_masm.clone(), &args).first().unwrap().clone().into();
                // prop_assert_eq!(rust_out, emul_out);
                Ok(())
            })
            .unwrap();
    */
}

#[test]
fn function_call_hir2() {
    let name = "function_call_hir2";
    let cargo_proj = project(name)
        .file(
            "Cargo.toml",
            format!(
                r#"
                [package]
                name = "{name}"
                version = "0.0.1"
                edition = "2021"
                authors = []

                [lib]
                crate-type = ["cdylib"]

                [profile.release]
                # optimize the output for size
                opt-level = "z"
                panic = "abort"

                [profile.dev]
                panic = "abort"
                opt-level = 1
                debug-assertions = true
                overflow-checks = false
                debug = true
            "#,
            )
            .as_str(),
        )
        .file(
            "src/lib.rs",
            r#"
                #![no_std]

                // Global allocator to use heap memory in no-std environment
                // #[global_allocator]
                // static ALLOC: miden::BumpAlloc = miden::BumpAlloc::new();

                // Required for no-std crates
                #[panic_handler]
                fn my_panic(_info: &core::panic::PanicInfo) -> ! {
                    loop {}
                }

                // use miden::Felt;

                #[no_mangle]
                #[inline(never)]
                pub fn add(a: u32, b: u32) -> u32 {
                    a + b
                }

                #[no_mangle]
                pub fn entrypoint(a: u32, b: u32) -> u32 {
                    add(a, b)
                }
            "#,
        )
        .build();
    let mut test = CompilerTestBuilder::rust_source_cargo_miden(
        cargo_proj.root(),
        WasmTranslationConfig::default(),
        [],
    )
    .build();

    let artifact_name = name;
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir2(expect_file![format!("../../expected/{artifact_name}.hir")]);
}

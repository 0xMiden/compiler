use std::collections::VecDeque;

use expect_test::expect_file;
use midenc_debug::{Executor, PopFromStack, PushToStack};
use midenc_frontend_wasm2::WasmTranslationConfig;
use midenc_hir2::Felt;
use prop::test_runner::{Config, TestRunner};
use proptest::prelude::*;

use crate::{cargo_proj::project, CompilerTest, CompilerTestBuilder};

#[test]
fn fibonacci() {
    fn expected_fib(n: u32) -> u32 {
        let mut a = 0;
        let mut b = 1;
        for _ in 0..n {
            let c = a + b;
            a = b;
            b = c;
        }
        a
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/fibonacci",
        config,
        ["--entrypoint=fibonacci::entrypoint".into()],
    );
    test.expect_wasm(expect_file!["../../expected/examples/fib.wat"]);
    test.expect_ir(expect_file!["../../expected/examples/fib.hir"]);
    test.expect_masm(expect_file!["../../expected/examples/fib.masm"]);
    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::default()
        .run(&(1u32..30), move |a| {
            let rust_out = expected_fib(a);
            let mut args = Vec::<Felt>::default();
            PushToStack::try_push(&a, &mut args);

            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
}

#[test]
fn collatz() {
    let _ = env_logger::builder().is_test(true).try_init();

    fn expected(mut n: u32) -> u32 {
        let mut steps = 0;
        while n != 1 {
            if n % 2 == 0 {
                n /= 2;
            } else {
                n = 3 * n + 1;
            }
            steps += 1;
        }
        steps
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden(
        "../../examples/collatz",
        config,
        ["--entrypoint=collatz::entrypoint".into()],
    );
    let artifact_name = "collatz";
    test.expect_wasm(expect_file![format!("../../expected/{artifact_name}.wat")]);
    test.expect_ir(expect_file![format!("../../expected/{artifact_name}.hir")]);
    test.expect_masm(expect_file![format!("../../expected/{artifact_name}.masm")]);
    let package = test.compiled_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::new(Config::with_cases(4))
        .run(&(1u32..30), move |a| {
            let rust_out = expected(a);
            let mut args = Vec::<Felt>::default();
            PushToStack::try_push(&a, &mut args);

            let exec = Executor::for_package(&package, args, &test.session)
                .map_err(|err| TestCaseError::fail(err.to_string()))?;
            let output: u32 = exec.execute_into(&package.unwrap_program(), &test.session);
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| {
            panic!("{err}");
        });
}

use miden_debug::ToMidenRepr;
use midenc_frontend_wasm::WasmTranslationConfig;
use prop::test_runner::{Config, TestRunner};
use proptest::prelude::*;

use crate::{CompilerTest, testing::executor_with_std};

#[test]
fn collatz() {
    crate::testing::setup::enable_compiler_instrumentation();

    fn expected(mut n: u32) -> u32 {
        let mut steps = 0;
        while n != 1 {
            if n.is_multiple_of(2) {
                n /= 2;
            } else {
                n = 3 * n + 1;
            }
            steps += 1;
        }
        steps
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/collatz", config, []);
    let package = test.compile_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::new(Config::with_cases(4))
        .run(&(1u32..30), move |a| {
            let rust_out = expected(a);
            let args = a.to_felts().to_vec();
            let exec = executor_with_std(args, Some(&package));
            let output: u32 =
                exec.execute_into(&package.unwrap_program(), test.session.source_manager.clone());
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| {
            panic!("{err}");
        });
}

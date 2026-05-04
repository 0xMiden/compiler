use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;
use prop::test_runner::TestRunner;
use proptest::prelude::*;

use crate::{CompilerTest, testing::executor_with_std};

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
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/fibonacci", config, []);
    let package = test.compile_package();

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::default()
        .run(&(1u32..30), move |a| {
            let rust_out = expected_fib(a);
            let exec = executor_with_std(vec![Felt::new(a as u64)], Some(&package));
            let output: u32 =
                exec.execute_into(&package.unwrap_program(), test.session.source_manager.clone());
            dbg!(output);
            prop_assert_eq!(rust_out, output);
            Ok(())
        })
        .unwrap_or_else(|err| panic!("{err}"));
}

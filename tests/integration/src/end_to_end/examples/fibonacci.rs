use midenc_frontend_wasm::WasmTranslationConfig;
use midenc_hir::Felt;

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

    // Exhaust the small domain once, including the zero boundary.
    for a in 0u32..30 {
        let rust_out = expected_fib(a);
        let exec = executor_with_std(vec![Felt::new_unchecked(a as u64)]);
        let output: u32 = exec.execute_into(package.clone(), test.session.source_manager.clone());
        assert_eq!(rust_out, output, "fibonacci({a})");
    }
}

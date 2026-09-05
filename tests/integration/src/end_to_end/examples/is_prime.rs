// Compare MASM execution and retained HIR evaluation against the same Rust result.
#[test]
fn is_prime() {
    use miden_debug::ToMidenRepr;
    use midenc_frontend_wasm::WasmTranslationConfig;
    use midenc_hir::{Immediate, Op, SymbolTable};

    use crate::{CompilerTest, testing::executor_with_std};

    crate::testing::setup::enable_compiler_instrumentation();

    fn expected(n: u32) -> bool {
        if n <= 1 {
            return false;
        }
        if n <= 3 {
            return true;
        }
        if n.is_multiple_of(2) || n.is_multiple_of(3) {
            return false;
        }
        let mut i = 5;
        while i * i <= n {
            if n.is_multiple_of(i) || n.is_multiple_of(i + 2) {
                return false;
            }
            i += 6;
        }
        true
    }

    let config = WasmTranslationConfig::default();
    let mut test = CompilerTest::rust_source_cargo_miden("../../examples/is-prime", config, []);
    let package = test.compile_package();
    let hir = test.hir();

    // Exhaust the small domain once, including zero and one.
    for a in 0u32..30 {
        let rust_out = expected(a);

        // Test the IR
        let mut evaluator =
            midenc_hir_eval::HirEvaluator::new(hir.borrow().as_operation().context_rc());
        let op = hir
            .borrow()
            .symbol_manager()
            .lookup_symbol_ref(
                &midenc_hir::SymbolPath::new([
                    midenc_hir::SymbolNameComponent::Component("is_prime".into()),
                    midenc_hir::SymbolNameComponent::Leaf("entrypoint".into()),
                ])
                .unwrap(),
            )
            .unwrap();
        let result = evaluator
            .eval(&op.borrow(), [midenc_hir_eval::Value::Immediate((a as i32).into())])
            .unwrap_or_else(|err| panic!("{err}"));
        let midenc_hir_eval::Value::Immediate(Immediate::I32(result)) = result[0] else {
            panic!("expected i32 immediate for input {a}, got {:?}", result[0]);
        };
        assert_eq!(rust_out as i32, result, "HIR is_prime({a})");

        let args = a.to_felts().to_vec();
        let exec = executor_with_std(args);
        let output: u32 = exec.execute_into(package.clone(), test.session.source_manager.clone());
        assert_eq!(rust_out as u32, output, "MASM is_prime({a})");
    }
}

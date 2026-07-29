// This test wants *two* artifacts from one build: the assembled package, and the optimized HIR
// component, which it evaluates with `HirEvaluator` and compares against the Rust original.
//
// It was parked for the length of the pipeline redesign, because a Cargo fixture compiles through
// `RUST_FRONTEND`, whose route was `[package.assembled]` alone: the root target was built by
// recursing with its own `Session` and `Context`, so the outer run's observers never saw
// `hir.transformed`. The root arm now runs the shared WebAssembly tail in this process and
// publishes every checkpoint on that route, and `CompilerTest::hir` retains the run's `Context`
// so the component is still evaluable after the run returns — HIR operations hold only a raw
// pointer to the arena they were allocated in.
#[test]
fn is_prime() {
    use miden_debug::ToMidenRepr;
    use midenc_frontend_wasm::WasmTranslationConfig;
    use midenc_hir::{Immediate, Op, SymbolTable};
    use prop::test_runner::{Config, TestRunner};
    use proptest::prelude::*;

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

    println!("{}", hir.borrow().as_operation());

    // Run the Rust and compiled MASM code against a bunch of random inputs and compare the results
    TestRunner::new(Config::with_cases(100))
        .run(&(1u32..30), move |a| {
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
                //return Err(TestCaseError::fail(format!(
                panic!("expected i32 immediate for input {a}, got {:?}", result[0]);
                //)));
            };
            prop_assert_eq!(rust_out as i32, result);

            let args = a.to_felts().to_vec();
            let exec = executor_with_std(args);
            let output: u32 =
                exec.execute_into(package.clone(), test.session.source_manager.clone());
            dbg!(output);
            prop_assert_eq!(rust_out as u32, output);
            Ok(())
        })
        .unwrap_or_else(|err| {
            panic!("{err}");
        });
}

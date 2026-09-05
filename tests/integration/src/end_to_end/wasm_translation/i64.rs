use std::cell::RefCell;

use miden_debug::{FromMidenRepr, ToMidenRepr};
use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};
use proptest::{prelude::*, test_runner::TestCaseError};

use super::wasm_interpreter::WasmInterpreter;
use crate::{
    CompilerTestBuilder,
    end_to_end::support::{NumericStrategy, TrapExpectation, default_host_with_core_lib},
};

#[test]
fn i64_rem_s() {
    let wasm = wat::parse_str(
        r#"(module
            (func $entrypoint (export "entrypoint") (param i64 i64) (result i64)
                local.get 0 local.get 1 i64.rem_s))"#,
    )
    .unwrap();
    let interpreter = RefCell::new(WasmInterpreter::new(&wasm));
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let program = builder.build().compile_package().unwrap_program();

    // Includes MIN % -1, MIN as divisor, zero divisors, and arbitrary signed operands.
    NumericStrategy::<i64>::rem_signed_checked().run(|(a, b)| {
        let expected = interpreter
            .borrow_mut()
            .call_entrypoint::<(i64, i64), i64>("entrypoint", (a, b));
        let mut inputs = Vec::with_capacity(4);
        a.push_to_operand_stack(&mut inputs);
        b.push_to_operand_stack(&mut inputs);
        let actual = execute_sync(
            &program,
            StackInputs::new(&inputs).unwrap(),
            AdviceInputs::default(),
            &mut default_host_with_core_lib(),
            ExecutionOptions::default(),
        );
        match (expected, actual) {
            (Ok(expected), Ok(trace)) => {
                prop_assert_eq!(i64::from_felts(trace.stack.get_num_elements(2)), expected);
                Ok(())
            }
            (Err(wasm_err), Err(vm_err)) => TrapExpectation::try_from(&wasm_err)
                .map_err(TestCaseError::fail)?
                .check(&vm_err)
                .map_err(TestCaseError::fail),
            (expected, actual) => Err(TestCaseError::fail(format!(
                "Wasm and Miden execution disagree for ({a}, {b}): {expected:?}, {actual:?}"
            ))),
        }
    });
}

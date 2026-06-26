use std::cell::RefCell;

use miden_core::Felt;
use miden_processor::{ExecutionOptions, StackInputs, advice::AdviceInputs, execute_sync};
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};
use proptest::{
    prelude::*,
    test_runner::{TestCaseError, TestError},
};

use super::wasm_interpreter::WasmInterpreter;
use crate::{
    CompilerTestBuilder,
    end_to_end::support::{NumericStrategy, TrapExpectation, default_host_with_core_lib},
};

/// Proptest harness for a binary `(i32, i32) -> i32` Wasm operation.
///
/// The `wat_op` is executed in a function exported as `entrypoint`.
fn test_i32_wasm_op_binary<S>(wat_op: &str, strategy: S)
where
    S: Strategy<Value = (i32, i32)>,
{
    let wat = format!(
        r#"(module
  (func $entrypoint (export "entrypoint") (param $a i32) (param $b i32) (result i32)
    local.get $a
    local.get $b
    {wat_op}
  )
)"#
    );
    let wasm = wat::parse_str(&wat).expect("failed to parse WAT module");

    // Executing a function requires mutable access to the interpreter's store but the proptest
    // closure is `Fn`.
    let interpreter = RefCell::new(WasmInterpreter::new(&wasm));

    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let mut test = builder.build();
    let package = test.compile_package();
    let program = package.unwrap_program();

    let res = NumericStrategy::<i32>::test_runner().run(&strategy, |(a, b)| {
        let expected = interpreter
            .borrow_mut()
            .call_entrypoint::<(i32, i32), i32>("entrypoint", (a, b));

        // The `(a, b)` entrypoint follows the C calling convention, so pass stack `[a, b]`
        let stack_inputs = StackInputs::new(&[
            Felt::new(a as u32 as u64).expect("u32 values fit in a felt"),
            Felt::new(b as u32 as u64).expect("u32 values fit in a felt"),
        ])
        .expect("invalid stack inputs");

        let vm_result = execute_sync(
            &program,
            stack_inputs,
            AdviceInputs::default(),
            &mut default_host_with_core_lib(),
            ExecutionOptions::default(),
        );

        match (expected, vm_result) {
            (Ok(expected), Ok(output)) => {
                let outputs: Vec<i32> = output
                    .stack
                    .get_num_elements(1)
                    .iter()
                    .map(|f| f.as_canonical_u64() as u32 as i32)
                    .collect();
                prop_assert_eq!(outputs, vec![expected]);
                Ok(())
            }
            (Err(wasm_err), Err(vm_err)) => {
                let expected_trap =
                    TrapExpectation::try_from(&wasm_err).map_err(TestCaseError::fail)?;
                expected_trap.check(&vm_err).map_err(TestCaseError::fail)
            }
            (Ok(expected), Err(vm_err)) => Err(TestCaseError::fail(format!(
                "expected Miden VM to return {expected}, but it trapped: {vm_err}"
            ))),
            (Err(wasm_err), Ok(output)) => {
                let outputs: Vec<i32> =
                    output.stack.iter().map(|f| f.as_canonical_u64() as u32 as i32).collect();
                Err(TestCaseError::fail(format!(
                    "expected Miden VM to trap ({wasm_err}), but it returned {outputs:?}"
                )))
            }
        }
    });

    match res {
        Err(TestError::Fail(reason, value)) => {
            panic!("Found minimal failing case: {value:?}\n{reason}")
        }
        Ok(_) => (),
        _ => panic!("Unexpected test result: {:?}", res),
    }
}

#[test]
fn i32_add() {
    test_i32_wasm_op_binary("i32.add", NumericStrategy::<i32>::add_signed());
}

#[test]
fn i32_div_s() {
    test_i32_wasm_op_binary("i32.div_s", NumericStrategy::<i32>::div_signed_checked());
}

#[test]
#[ignore = "https://github.com/0xMiden/compiler/issues/1206"]
fn i32_rem_s() {
    test_i32_wasm_op_binary("i32.rem_s", NumericStrategy::<i32>::rem_signed_checked());
}

/// Convert a [`wasmi`] trap into the [`TrapExpectation`] describing the matching Miden VM trap.
impl<'a> TryFrom<&'a wasmi::Error> for TrapExpectation {
    type Error = String;

    fn try_from(err: &'a wasmi::Error) -> Result<Self, Self::Error> {
        use wasmi::TrapCode;
        match err.as_trap_code() {
            Some(TrapCode::IntegerDivisionByZero) => Ok(TrapExpectation::DivideByZero),
            Some(TrapCode::IntegerOverflow) => Ok(TrapExpectation::FailedAssertionOverflow),
            Some(other) => Err(format!(
                "no Miden VM trap mapped for wasmi trap {:?}: {}",
                other,
                other.trap_message()
            )),
            None => Err(format!("wasmi trapped without a trap code: {err:?}")),
        }
    }
}

use std::cell::RefCell;

use miden_core::Felt;
use miden_debug::{FromMidenRepr, ToMidenRepr};
use miden_processor::{FastProcessor, StackInputs};
use midenc_hir::{FunctionIdent, Ident, interner::Symbol};
use proptest::{prelude::*, test_runner::TestCaseError};

use super::wasm_interpreter::WasmInterpreter;
use crate::{
    CompilerTestBuilder,
    end_to_end::support::{NumericStrategy, TrapExpectation, default_host_with_core_lib},
};

/// Checks a unary `(i64) -> i64` Wasm operation sequence against the Wasm interpreter.
///
/// The `wat_ops` are executed with the parameter on the Wasm operand stack.
fn check_i64_wasm_unary_ops(wat_ops: &str) {
    let wat = format!(
        r#"(module
            (func $entrypoint (export "entrypoint") (param i64) (result i64)
                local.get 0 {wat_ops}))"#
    );
    let wasm = wat::parse_str(&wat).unwrap();
    let mut interpreter = WasmInterpreter::new(&wasm);
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let program = builder.build().compile_package().unwrap_program();

    // Limbs are distinct in every non-trivial case, so a limb mix-up cannot go unnoticed.
    for value in [0, 1, -1, i64::MIN, i64::MAX, 0x0000_0002_0000_0005, 0x1234_5678_9abc_def0] {
        let expected = interpreter
            .call_entrypoint::<i64, i64>("entrypoint", value)
            .expect("Wasm execution failed");
        let mut inputs = Vec::with_capacity(2);
        value.push_to_operand_stack(&mut inputs);
        let output = FastProcessor::new(StackInputs::new(&inputs).unwrap())
            .execute_sync(&program, &mut default_host_with_core_lib())
            .expect("Miden execution failed");
        assert_eq!(
            i64::from_felts(output.stack.get_num_elements(2)),
            expected,
            "Wasm and Miden execution disagree for `{wat_ops}` of {value:#x}"
        );
    }
}

/// A 64-bit shift or rotate by a constant 32 bits is a move of whole 32-bit limbs.
#[test]
fn i64_whole_limb_moves() {
    check_i64_wasm_unary_ops("i64.const 32 i64.shr_u");
    check_i64_wasm_unary_ops("i64.const 32 i64.shl");
    check_i64_wasm_unary_ops("i64.const 32 i64.rotl");
    check_i64_wasm_unary_ops("i64.const 32 i64.rotr");
    // The count wraps modulo 64, so these are the same whole-limb moves.
    check_i64_wasm_unary_ops("i64.const 96 i64.shr_u");
    check_i64_wasm_unary_ops("i64.const -32 i64.rotl");
}

/// Runs `wat_ops` after storing the two `f32` (felt) parameters into adjacent 4-byte cells at
/// addresses 1024 and 1028, and returns the felt the ops leave on the Wasm operand stack.
fn run_felt_cells_wasm_ops(wat_ops: &str, first: Felt, second: Felt) -> Felt {
    let wat = format!(
        r#"(module
            (memory 1)
            (func $entrypoint (export "entrypoint") (param f32 f32) (result f32)
                i32.const 1024 local.get 0 f32.store
                i32.const 1028 local.get 1 f32.store
                {wat_ops}))"#
    );
    let wasm = wat::parse_str(&wat).unwrap();
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, []);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let program = builder.build().compile_package().unwrap_program();

    let mut inputs = Vec::with_capacity(2);
    first.push_to_operand_stack(&mut inputs);
    second.push_to_operand_stack(&mut inputs);
    let output = FastProcessor::new(StackInputs::new(&inputs).unwrap())
        .execute_sync(&program, &mut default_host_with_core_lib())
        .expect("Miden execution failed");
    output.stack.get_num_elements(1)[0]
}

/// Two felt cells moved through memory as one `i64` keep their full values: the high cell
/// extracted with a shift by 32, and the cells swapped with a rotate by 32.
#[test]
fn i64_moves_of_felt_cells() {
    // Felts that do not fit in 32 bits and differ, so a truncated or swapped cell is visible.
    let pairs = [
        (3_000_000_000_000, 1_000_000_000_000),
        (0xffff_ffff_0000_0000 - 7, 3_000_000_000_000),
    ];
    for (first, second) in pairs.map(|(a, b)| (Felt::new_unchecked(a), Felt::new_unchecked(b))) {
        let shr = "i32.const 1024 i64.load i64.const 32 i64.shr_u i32.wrap_i64 f32.reinterpret_i32";
        assert_eq!(run_felt_cells_wasm_ops(shr, first, second), second, "high cell via shr_u");

        let rotl = "i32.const 1024 i32.const 1024 i64.load i64.const 32 i64.rotl i64.store";
        let low = format!("{rotl} i32.const 1024 f32.load");
        assert_eq!(run_felt_cells_wasm_ops(&low, first, second), second, "low cell after rotl");
        let high = format!("{rotl} i32.const 1028 f32.load");
        assert_eq!(run_felt_cells_wasm_ops(&high, first, second), first, "high cell after rotl");
    }
}

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
        let actual = FastProcessor::new(StackInputs::new(&inputs).unwrap())
            .execute_sync(&program, &mut default_host_with_core_lib());
        match (expected, actual) {
            (Ok(expected), Ok(output)) => {
                prop_assert_eq!(i64::from_felts(output.stack.get_num_elements(2)), expected);
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

use std::sync::Arc;

use miden_debug::ToMidenRepr;
use miden_mast_package::Package;
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_wasm::{WasmMemArg, WasmOpBuilder, prepare_addr};
use midenc_hir::{
    Felt, FunctionIdent, Ident, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder,
    interner::Symbol,
};

use crate::{
    CompilerTest, CompilerTestBuilder,
    testing::{Initializer, compile_test_module, compile_test_module_with_masm, eval_package},
};

mod aligned_memory;
mod i32_extend16_s;
mod i32_extend8_s;
mod i32_load16_s;
mod i32_load8_s;
mod i64_extend16_s;
mod i64_extend32_s;
mod i64_extend8_s;
mod i64_load16_s;
mod i64_load32_s;
mod i64_load8_s;
mod shifts;

fn assert_single_output(expected: u64, outputs: Vec<u64>) {
    let rest_all_zero = outputs.iter().skip(1).all(|&x| x == 0);
    assert!(
        rest_all_zero,
        "expected all elements after first to be zero, but got {outputs:?}"
    );

    assert_eq!(outputs[0], expected, "actual {:#b}, expected {:#b}", outputs[0], expected);
}

/// Compile a Wasm module whose fields are `fields` (one memory page is declared), with the function
/// exported as `entrypoint` as the program entrypoint, and the test harness that applies memory
/// initializers. Returns the test, its package, and the emitted MASM of the module.
fn compile_wat(fields: &str) -> (CompilerTest, Arc<Package>, String) {
    let wasm = wat::parse_str(format!("(module (memory 1) {fields})")).unwrap();
    let mut builder = CompilerTestBuilder::from_wasm("test", wasm, ["--test-harness".into()]);
    builder.with_entrypoint(FunctionIdent {
        module: Ident::with_empty_span(Symbol::intern("test")),
        function: Ident::with_empty_span(Symbol::intern("entrypoint")),
    });
    let mut test = builder.build();
    let masm = test.masm_src();
    let package = test.compile_package();
    (test, package, masm)
}

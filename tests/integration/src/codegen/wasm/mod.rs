use miden_debug::ToMidenRepr;
use midenc_dialect_wasm::WasmOpBuilder;
use midenc_hir::{Felt, SourceSpan, Type, ValueRef, dialects::builtin::BuiltinOpBuilder};

use crate::testing::{Initializer, compile_test_module, eval_package};

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

fn assert_single_output(expected: u64, outputs: Vec<u64>) {
    let rest_all_zero = outputs.iter().skip(1).all(|&x| x == 0);
    assert!(
        rest_all_zero,
        "expected all elements after first to be zero, but got {outputs:?}"
    );

    assert_eq!(outputs[0], expected, "actual {:#b}, expected {:#b}", outputs[0], expected);
}

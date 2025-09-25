use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::FunctionRef,
    interner::{symbols, Symbol},
    Builder, FunctionType, SmallVec, SourceSpan, SymbolNameComponent, Type, ValueRef,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_ID: &str = "intrinsics::advice";
/// The module path prefix for advice intrinsics, not including the function name
pub const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Advice),
];

/// Get the [FunctionType] of an advice intrinsic, if it is implemented as a function.
///
/// Returns `None` for intrinsics which are unknown, or correspond to native instructions.
pub fn function_type(function: Symbol) -> Option<FunctionType> {
    match function.as_str() {
        "adv_push_mapvaln" => {
            // The WASM import signature: takes 4 f32 values (Word) and returns 1 f32
            let sig = FunctionType::new(
                midenc_hir::CallConv::Wasm,
                vec![
                    Type::Felt, // key0
                    Type::Felt, // key1
                    Type::Felt, // key2
                    Type::Felt, // key3
                ],
                vec![Type::Felt], // Returns number of elements pushed
            );
            Some(sig)
        }
        "adv_insert_mem" => {
            // Signature: (key0..key3, start_ptr, end_ptr) -> ()
            Some(FunctionType::new(
                midenc_hir::CallConv::Wasm,
                vec![Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::Felt],
                vec![],
            ))
        }
        "emit_falcon_sig_to_stack" => {
            // (msg0..msg3, pk0..pk3) -> ()
            Some(FunctionType::new(
                midenc_hir::CallConv::Wasm,
                vec![
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                    Type::Felt,
                ],
                vec![],
            ))
        }
        _ => None,
    }
}

/// Convert a call to an advice intrinsic function into instruction(s)
pub fn convert_advice_intrinsics<B: ?Sized + Builder>(
    function: Symbol,
    function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    let function_ref =
        function_ref.unwrap_or_else(|| panic!("expected '{function}' to have been declared"));

    match function.as_str() {
        "adv_push_mapvaln" => {
            // The WASM import has 4 parameters (key0-3) and returns 1 f32
            assert_eq!(args.len(), 4, "{function} takes exactly four arguments (key0-3)");

            let func = function_ref.borrow();
            let signature = func.signature().clone();
            drop(func);

            // Call the function with all arguments
            // The intrinsics::advice::adv_push_mapvaln function will be mapped to the MASM adv_push_mapvaln
            let exec = builder.exec(function_ref, signature, args.iter().copied(), span)?;

            // Extract the return value from the exec operation
            let borrow = exec.borrow();
            let results = borrow.as_ref().results();
            let result_vals: SmallVec<[ValueRef; 1]> =
                results.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

            // The function returns the number of elements pushed as i32
            Ok(result_vals)
        }
        "emit_falcon_sig_to_stack" => {
            assert_eq!(args.len(), 8, "{function} takes exactly eight arguments");
            let func = function_ref.borrow();
            let signature = func.signature().clone();
            drop(func);
            let _ = builder.exec(function_ref, signature, args.iter().copied(), span)?;
            Ok(SmallVec::new())
        }
        "adv_insert_mem" => {
            // Lower to MASM intrinsic call: intrinsics::advice::insert_mem
            assert_eq!(args.len(), 6, "insert_mem takes exactly six arguments");
            let func = function_ref.borrow();
            let signature = func.signature().clone();
            drop(func);
            let _ = builder.exec(function_ref, signature, args.iter().copied(), span)?;
            Ok(SmallVec::new())
        }
        _ => {
            panic!("unsupported io intrinsic: '{function}'")
        }
    }
}

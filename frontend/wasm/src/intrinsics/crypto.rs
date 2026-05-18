//! Cryptographic intrinsics conversion module for WebAssembly to Miden IR.
//!
//! This module handles the conversion of cryptographic operations from Wasm imports
//! to their corresponding Miden VM instructions.

use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, FunctionType, SmallVec, SourceSpan, SymbolNameComponent, Type, ValueRef,
    dialects::builtin::FunctionRef,
    interner::{Symbol, symbols},
    smallvec,
};

use super::{IntrinsicEffect, IntrinsicsConversionResult};
use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Crypto),
];

/// Get the [FunctionType] of a crypto intrinsic, if it is implemented as a function.
///
/// Returns `None` for intrinsics which are unknown, or correspond to native instructions.
pub fn function_type(function: Symbol) -> Option<FunctionType> {
    match function.as_str() {
        "hmerge" => {
            // The WASM import signature: takes 2 i32 pointers (digests array pointer + result pointer)
            let sig = midenc_hir::FunctionType::new(
                midenc_hir::CallConv::Wasm,
                vec![
                    // Pointer to array of two digests
                    Type::I32,
                    // Result pointer
                    Type::I32,
                ],
                vec![], // No returns - writes to the result pointer
            );
            Some(sig)
        }
        _ => None,
    }
}

pub fn function_effects(function: Symbol) -> Option<SmallVec<[IntrinsicEffect; 2]>> {
    match function.as_str() {
        "hmerge" => Some(smallvec![]),
        _ => None,
    }
}

pub fn as_intrinsic(function: Symbol) -> Option<IntrinsicsConversionResult> {
    let ty = function_type(function)?;
    let effects = function_effects(function)?;

    Some(IntrinsicsConversionResult::FunctionType { ty, effects })
}

/// Convert a call to a crypto intrinsic function into instruction(s)
pub(crate) fn convert_crypto_intrinsics<B: ?Sized + Builder>(
    function: Symbol,
    function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    let function_ref =
        function_ref.unwrap_or_else(|| panic!("expected '{function}' to have been declared"));

    match function.as_str() {
        "hmerge" => {
            // The WASM import has 2 parameters (digests pointer + result pointer)
            assert_eq!(
                args.len(),
                2,
                "{function} takes exactly two arguments (digests pointer + result pointer)"
            );

            let func = function_ref.borrow();
            let signature = func.get_signature().clone();
            drop(func);

            // Call the function with both arguments
            // The intrinsics::crypto::hmerge function will be mapped to the MASM hmerge
            let _exec = builder.exec(function_ref, signature, args.iter().copied(), span)?;

            // Since the WASM signature has the result pointer as the last parameter,
            // the function doesn't return anything - it writes to memory
            Ok(SmallVec::new())
        }
        unknown => panic!("unknown crypto intrinsic: {unknown}"),
    }
}

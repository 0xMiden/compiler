use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::FunctionRef,
    interner::{symbols, Symbol},
    Builder, SmallVec, SourceSpan, SymbolNameComponent, ValueRef,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_ID: &str = "intrinsics::crypto";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Crypto),
];

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
            // The WASM import has 9 parameters (8 felts + result pointer)
            assert_eq!(
                args.len(),
                9,
                "{function} takes exactly nine arguments (8 digest values + result pointer)"
            );

            let func = function_ref.borrow();
            let signature = func.signature().clone();
            drop(func);

            // Call the function with all 9 arguments
            // The intrinsics::crypto::hmerge function will be mapped to the MASM hmerge_ptr
            let _exec = builder.exec(function_ref, signature, args.iter().copied(), span)?;

            // Since the WASM signature has the result pointer as the last parameter,
            // the function doesn't return anything - it writes to memory
            Ok(SmallVec::new())
        }
        unknown => panic!("unknown crypto intrinsic: {unknown}"),
    }
}

use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, SmallVec, SourceSpan, SymbolNameComponent, ValueRef,
    dialects::builtin::FunctionRef,
    interner::{Symbol, symbols},
    smallvec,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Debug),
];

/// Convert a call to a debugging intrinsic function into instruction(s)
pub(crate) fn convert_debug_intrinsics<B: ?Sized + Builder>(
    function: Symbol,
    _function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match function.as_str() {
        "break" => {
            assert_eq!(args.len(), 0, "{function} takes exactly one argument");
            builder.breakpoint(span)?;
            Ok(smallvec![])
        }
        _ => panic!("no debug intrinsics found named '{function}'"),
    }
}

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
    _builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match function.as_str() {
        "break" => {
            assert_eq!(args.len(), 0, "{function} takes no arguments");
            // VM v0.22 no longer exposes a breakpoint instruction, so debug breakpoints compile
            // to a no-op until we have another debugger hook to target.
            let _ = span;
            Ok(smallvec![])
        }
        _ => panic!("no debug intrinsics found named '{function}'"),
    }
}

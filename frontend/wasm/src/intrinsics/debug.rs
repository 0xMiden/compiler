use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, PointerType, SmallVec, SourceSpan, SymbolNameComponent, Type, ValueRef,
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
            assert_eq!(args.len(), 0, "{function} takes no arguments");
            // VM v0.22 no longer exposes a breakpoint instruction, so debug breakpoints compile
            // to a no-op until we have another debugger hook to target.
            let _ = span;
            Ok(smallvec![])
        }
        "println" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let ptr_ty = Type::from(PointerType::new(Type::U8));
            let ptr = builder.inttoptr(args[0], ptr_ty, span)?;
            let len = builder.bitcast(args[1], Type::U32, span)?;
            builder.println(ptr, len, span)?;
            Ok(smallvec![])
        }
        _ => panic!("no debug intrinsics found named '{function}'"),
    }
}

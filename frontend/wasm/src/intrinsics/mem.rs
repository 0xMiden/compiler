use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::FunctionRef,
    interner::{symbols, Symbol},
    AbiParam, Builder, CallConv, FunctionType, Signature, SmallVec, SourceSpan,
    SymbolNameComponent, Type, ValueRef,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_ID: &str = "intrinsics::mem";
pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::Mem),
];

pub const HEAP_BASE: &str = "heap_base";

const HEAP_BASE_FUNC: ([Type; 0], [Type; 1]) = ([], [Type::U32]);

pub fn function_type(function: Symbol) -> Option<FunctionType> {
    match function.as_str() {
        HEAP_BASE => Some(FunctionType::new(CallConv::Wasm, HEAP_BASE_FUNC.0, HEAP_BASE_FUNC.1)),
        _ => None,
    }
}

fn signature(function: Symbol) -> Signature {
    match function.as_str() {
        HEAP_BASE => {
            Signature::new(HEAP_BASE_FUNC.0.map(AbiParam::new), HEAP_BASE_FUNC.1.map(AbiParam::new))
        }
        _ => panic!("No memory intrinsics Signature found for {function}"),
    }
}

/// Convert a call to a memory intrinsic function
pub(crate) fn convert_mem_intrinsics<B: ?Sized + Builder>(
    function: Symbol,
    function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    let function_ref =
        function_ref.unwrap_or_else(|| panic!("expected '{function}' to have been declared"));
    match function.as_str() {
        HEAP_BASE => {
            let func = function_ref.borrow();
            assert_eq!(args.len(), 0, "{} takes no arguments", &func.name());

            let signature = func.signature().clone();
            drop(func);
            let exec = builder.exec(function_ref, signature, args.iter().copied(), span)?;
            let borrow = exec.borrow();
            let results = borrow.as_ref().results();
            Ok(results.iter().map(|op_res| op_res.borrow().as_value_ref()).collect())
        }
        _ => panic!("no memory intrinsics found with name '{function}'"),
    }
}

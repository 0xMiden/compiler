use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    Builder, SmallVec, SourceSpan, SymbolNameComponent, Type, ValueRef,
    dialects::builtin::FunctionRef,
    interner::{Symbol, symbols},
    smallvec,
};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_PREFIX: &[SymbolNameComponent] = &[
    SymbolNameComponent::Root,
    SymbolNameComponent::Component(symbols::Intrinsics),
    SymbolNameComponent::Component(symbols::FeltModule),
];

/// Convert a call to a felt op intrinsic function into instruction(s)
pub(crate) fn convert_felt_intrinsics<B: ?Sized + Builder>(
    function: Symbol,
    _function_ref: Option<FunctionRef>,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 1]>> {
    match function.as_str() {
        // Conversion operations
        "from_u64_unchecked" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.cast(args[0], Type::Felt, span)?;
            Ok(smallvec![inst])
        }
        "from_u32" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.bitcast(args[0], Type::Felt, span)?;
            Ok(smallvec![inst])
        }
        "as_u64" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            // we're casting to i64 instead of u64 because Wasm doesn't have u64
            // and this value will be used in Wasm ops or local vars that expect i64
            let inst = builder.cast(args[0], Type::I64, span)?;
            Ok(smallvec![inst])
        }
        // Arithmetic operations
        "add" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.add_unchecked(args[0], args[1], span)?;
            Ok(smallvec![inst])
        }
        "sub" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.sub_unchecked(args[0], args[1], span)?;
            Ok(smallvec![inst])
        }
        "mul" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.mul_unchecked(args[0], args[1], span)?;
            Ok(smallvec![inst])
        }
        "div" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.div(args[0], args[1], span)?;
            Ok(smallvec![inst])
        }
        "neg" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.neg(args[0], span)?;
            Ok(smallvec![inst])
        }
        "inv" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.inv(args[0], span)?;
            Ok(smallvec![inst])
        }
        "pow2" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.pow2(args[0], span)?;
            Ok(smallvec![inst])
        }
        "exp" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.exp(args[0], args[1], span)?;
            Ok(smallvec![inst])
        }
        // Comparison operations
        "eq" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.eq(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        "gt" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.gt(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        "ge" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.gte(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        "lt" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.lt(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        "le" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            let inst = builder.lte(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        "is_odd" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            let inst = builder.is_odd(args[0], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(smallvec![cast])
        }
        // Assert operations
        "assert" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            builder.assert(args[0], span)?;
            Ok(smallvec![])
        }
        "assertz" => {
            assert_eq!(args.len(), 1, "{function} takes exactly one argument");
            builder.assertz(args[0], span)?;
            Ok(smallvec![])
        }
        "assert_eq" => {
            assert_eq!(args.len(), 2, "{function} takes exactly two arguments");
            builder.assert_eq(args[0], args[1], span)?;
            Ok(smallvec![])
        }
        _ => panic!("no felt intrinsics found named '{function}'"),
    }
}

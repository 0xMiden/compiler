use std::vec;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{Builder, FunctionIdent, SourceSpan, Type, ValueRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_ID: &str = "intrinsics::felt";

/// Convert a call to a felt op intrinsic function into instruction(s)
pub(crate) fn convert_felt_intrinsics<B: ?Sized + Builder>(
    func_id: FunctionIdent,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match func_id.function.as_symbol().as_str() {
        // Conversion operations
        "from_u64_unchecked" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.cast(args[0], Type::Felt, span)?;
            Ok(vec![inst])
        }
        "from_u32" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.bitcast(args[0], Type::Felt, span)?;
            Ok(vec![inst])
        }
        "as_u64" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            // we're casting to i64 instead of u64 because Wasm doesn't have u64
            // and this value will be used in Wasm ops or local vars that expect i64
            let inst = builder.cast(args[0], Type::I64, span)?;
            Ok(vec![inst])
        }
        // Arithmetic operations
        "add" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.add_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "sub" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.sub_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "mul" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.mul_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "div" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.div(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "neg" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.neg(args[0], span)?;
            Ok(vec![inst])
        }
        "inv" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.inv(args[0], span)?;
            Ok(vec![inst])
        }
        "pow2" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.pow2(args[0], span)?;
            Ok(vec![inst])
        }
        "exp" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.exp(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        // Comparison operations
        "eq" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.eq(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "gt" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.gt(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "ge" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.gte(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "lt" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.lt(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "le" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.lte(args[0], args[1], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "is_odd" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.is_odd(args[0], span)?;
            let cast = builder.cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        // Assert operations
        "assert" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            builder.assert(args[0], span)?;
            Ok(vec![])
        }
        "assertz" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            builder.assertz(args[0], span)?;
            Ok(vec![])
        }
        "assert_eq" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            builder.assert_eq(args[0], args[1], span)?;
            Ok(vec![])
        }
        _ => panic!("No felt op intrinsics found for {}", func_id),
    }
}

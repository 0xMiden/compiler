use std::vec;

use midenc_dialect_hir::InstBuilder;
use midenc_hir2::{FunctionIdent, SourceSpan, Type, ValueRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

pub(crate) const MODULE_ID: &str = "intrinsics::felt";

/// Convert a call to a felt op intrinsic function into instruction(s)
pub(crate) fn convert_felt_intrinsics(
    func_id: FunctionIdent,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    match func_id.function.as_symbol().as_str() {
        // Conversion operations
        "from_u64_unchecked" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().cast(args[0], Type::Felt, span)?;
            Ok(vec![inst])
        }
        "from_u32" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().bitcast(args[0], Type::Felt, span)?;
            Ok(vec![inst])
        }
        "as_u64" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            // we're casting to i64 instead of u64 because Wasm doesn't have u64
            // and this value will be used in Wasm ops or local vars that expect i64
            let inst = builder.ins().cast(args[0], Type::I64, span)?;
            Ok(vec![inst])
        }
        // Arithmetic operations
        "add" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().add_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "sub" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().sub_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "mul" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().mul_unchecked(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "div" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().div(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        "neg" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().neg(args[0], span)?;
            Ok(vec![inst])
        }
        "inv" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().inv(args[0], span)?;
            Ok(vec![inst])
        }
        "pow2" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().pow2(args[0], span)?;
            Ok(vec![inst])
        }
        "exp" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().exp(args[0], args[1], span)?;
            Ok(vec![inst])
        }
        // Comparison operations
        "eq" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().eq(args[0], args[1], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "gt" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().gt(args[0], args[1], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "ge" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().gte(args[0], args[1], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "lt" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().lt(args[0], args[1], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "le" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            let inst = builder.ins().lte(args[0], args[1], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        "is_odd" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            let inst = builder.ins().is_odd(args[0], span)?;
            let cast = builder.ins().cast(inst, Type::I32, span)?;
            Ok(vec![cast])
        }
        // Assert operations
        "assert" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            builder.ins().assert(args[0], span);
            Ok(vec![])
        }
        "assertz" => {
            assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
            builder.ins().assertz(args[0], span);
            Ok(vec![])
        }
        "assert_eq" => {
            assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
            builder.ins().assert_eq(args[0], args[1], span);
            Ok(vec![])
        }
        _ => panic!("No felt op intrinsics found for {}", func_id),
    }
}

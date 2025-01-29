use std::vec;

use midenc_hir::{FunctionIdent, InstBuilder, SourceSpan, Type::*, Value};
use midenc_hir2::ValueRef;

use crate::module::function_builder_ext::FunctionBuilderExt;

pub(crate) const MODULE_ID: &str = "intrinsics::felt";

/// Convert a call to a felt op intrinsic function into instruction(s)
pub(crate) fn convert_felt_intrinsics(
    func_id: FunctionIdent,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_>,
    span: SourceSpan,
) -> Vec<ValueRef> {
    todo!()
    // match func_id.function.as_symbol().as_str() {
    //     // Conversion operations
    //     "from_u64_unchecked" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().cast(args[0], Felt, span);
    //         vec![inst]
    //     }
    //     "from_u32" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().bitcast(args[0], Felt, span);
    //         vec![inst]
    //     }
    //     "as_u64" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         // we're casting to i64 instead of u64 because Wasm doesn't have u64
    //         // and this value will be used in Wasm ops or local vars that expect i64
    //         let inst = builder.ins().cast(args[0], I64, span);
    //         vec![inst]
    //     }
    //     // Arithmetic operations
    //     "add" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().add_unchecked(args[0], args[1], span);
    //         vec![inst]
    //     }
    //     "sub" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().sub_unchecked(args[0], args[1], span);
    //         vec![inst]
    //     }
    //     "mul" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().mul_unchecked(args[0], args[1], span);
    //         vec![inst]
    //     }
    //     "div" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().div_unchecked(args[0], args[1], span);
    //         vec![inst]
    //     }
    //     "neg" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().neg(args[0], span);
    //         vec![inst]
    //     }
    //     "inv" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().inv(args[0], span);
    //         vec![inst]
    //     }
    //     "pow2" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().pow2(args[0], span);
    //         vec![inst]
    //     }
    //     "exp" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().exp(args[0], args[1], span);
    //         vec![inst]
    //     }
    //     // Comparison operations
    //     "eq" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().eq(args[0], args[1], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     "gt" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().gt(args[0], args[1], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     "ge" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().gte(args[0], args[1], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     "lt" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().lt(args[0], args[1], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     "le" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         let inst = builder.ins().lte(args[0], args[1], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     "is_odd" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         let inst = builder.ins().is_odd(args[0], span);
    //         let cast = builder.ins().cast(inst, I32, span);
    //         vec![cast]
    //     }
    //     // Assert operations
    //     "assert" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         builder.ins().assert(args[0], span);
    //         vec![]
    //     }
    //     "assertz" => {
    //         assert_eq!(args.len(), 1, "{} takes exactly one argument", func_id);
    //         builder.ins().assertz(args[0], span);
    //         vec![]
    //     }
    //     "assert_eq" => {
    //         assert_eq!(args.len(), 2, "{} takes exactly two arguments", func_id);
    //         builder.ins().assert_eq(args[0], args[1], span);
    //         vec![]
    //     }
    //     _ => panic!("No felt op intrinsics found for {}", func_id),
    // }
}

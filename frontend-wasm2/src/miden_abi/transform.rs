use midenc_hir::{
    diagnostics::{DiagnosticsHandler, SourceSpan},
    FunctionIdent, Immediate, InstBuilder,
    Type::*,
    Value,
};

use super::{stdlib, tx_kernel};
use crate::module::function_builder_ext::FunctionBuilderExt;

/// The strategy to use for transforming a function call
enum TransformStrategy {
    /// The Miden ABI function returns a length and a pointer and we only want the length
    ListReturn,
    /// The Miden ABI function returns on the stack and we want to return via a pointer argument
    ReturnViaPointer,
    /// No transformation needed
    NoTransform,
}

/// Get the transformation strategy for a function name
fn get_transform_strategy(module_id: &str, function_id: &str) -> TransformStrategy {
    #[allow(clippy::single_match)]
    match module_id {
        stdlib::mem::MODULE_ID => match function_id {
            stdlib::mem::PIPE_WORDS_TO_MEMORY => return TransformStrategy::ReturnViaPointer,
            stdlib::mem::PIPE_DOUBLE_WORDS_TO_MEMORY => return TransformStrategy::ReturnViaPointer,
            _ => (),
        },
        stdlib::crypto::hashes::blake3::MODULE_ID => match function_id {
            stdlib::crypto::hashes::blake3::HASH_1TO1 => {
                return TransformStrategy::ReturnViaPointer
            }
            stdlib::crypto::hashes::blake3::HASH_2TO1 => {
                return TransformStrategy::ReturnViaPointer
            }
            _ => (),
        },
        stdlib::crypto::dsa::rpo_falcon::MODULE_ID => match function_id {
            stdlib::crypto::dsa::rpo_falcon::RPO_FALCON512_VERIFY => {
                return TransformStrategy::NoTransform
            }
            _ => (),
        },
        tx_kernel::note::MODULE_ID => match function_id {
            tx_kernel::note::GET_INPUTS => return TransformStrategy::ListReturn,
            _ => (),
        },
        tx_kernel::account::MODULE_ID => match function_id {
            tx_kernel::account::ADD_ASSET => return TransformStrategy::ReturnViaPointer,
            tx_kernel::account::REMOVE_ASSET => return TransformStrategy::ReturnViaPointer,
            tx_kernel::account::GET_ID => return TransformStrategy::NoTransform,
            _ => (),
        },
        tx_kernel::tx::MODULE_ID => match function_id {
            tx_kernel::tx::CREATE_NOTE => return TransformStrategy::NoTransform,
            _ => (),
        },
        _ => (),
    }
    panic!("No transform strategy found for function '{function_id}' in module '{module_id}'");
}

/// Transform a function call based on the transformation strategy
pub fn transform_miden_abi_call(
    func_id: FunctionIdent,
    args: &[Value],
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
    diagnostics: &DiagnosticsHandler,
) -> Vec<Value> {
    use TransformStrategy::*;
    match get_transform_strategy(func_id.module.as_str(), func_id.function.as_str()) {
        ListReturn => list_return(func_id, args, builder, span, diagnostics),
        ReturnViaPointer => return_via_pointer(func_id, args, builder, span, diagnostics),
        NoTransform => no_transform(func_id, args, builder, span, diagnostics),
    }
}

/// No transformation needed
#[inline(always)]
pub fn no_transform(
    func_id: FunctionIdent,
    args: &[Value],
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
    _diagnostics: &DiagnosticsHandler,
) -> Vec<Value> {
    let call = builder.ins().exec(func_id, args, span);
    let results = builder.inst_results(call);
    results.to_vec()
}

/// The Miden ABI function returns a length and a pointer and we only want the length
pub fn list_return(
    func_id: FunctionIdent,
    args: &[Value],
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
    _diagnostics: &DiagnosticsHandler,
) -> Vec<Value> {
    let call = builder.ins().exec(func_id, args, span);
    let results = builder.inst_results(call);
    assert_eq!(results.len(), 2, "List return strategy expects 2 results: length and pointer");
    // Return the first result (length) only
    results[0..1].to_vec()
}

/// The Miden ABI function returns felts on the stack and we want to return via a pointer argument
pub fn return_via_pointer(
    func_id: FunctionIdent,
    args: &[Value],
    builder: &mut FunctionBuilderExt,
    span: SourceSpan,
    _diagnostics: &DiagnosticsHandler,
) -> Vec<Value> {
    // Omit the last argument (pointer)
    let args_wo_pointer = &args[0..args.len() - 1];
    let call = builder.ins().exec(func_id, args_wo_pointer, span);
    let results = builder.inst_results(call).to_vec();
    let ptr_arg = *args.last().unwrap();
    let ptr_arg_ty = builder.data_flow_graph().value_type(ptr_arg).clone();
    assert_eq!(ptr_arg_ty, I32);
    let ptr_u32 = builder.ins().bitcast(ptr_arg, U32, span);
    let result_ty = midenc_hir::StructType::new(
        results.iter().map(|v| builder.data_flow_graph().value_type(*v).clone()),
    );
    for (idx, value) in results.iter().enumerate() {
        let value_ty = builder.data_flow_graph().value_type(*value).clone();
        let eff_ptr = if idx == 0 {
            // We're assuming here that the base pointer is of the correct alignment
            ptr_u32
        } else {
            let imm = Immediate::U32(result_ty.get(idx).offset);
            builder.ins().add_imm_checked(ptr_u32, imm, span)
        };
        let addr = builder.ins().inttoptr(eff_ptr, Ptr(value_ty.into()), span);
        builder.ins().store(addr, *value, span);
    }
    Vec::new()
}

use midenc_dialect_hir::InstBuilder;
use midenc_hir::diagnostics::{DiagnosticsHandler, SourceSpan};
use midenc_hir2::{dialects::builtin::FunctionRef, FunctionIdent, Immediate, Type, ValueRef};

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

/// Transform a Miden ABI function call based on the transformation strategy
///
/// `import_func` - import function that we're transforming a call to (think of a MASM function)
/// `args` - arguments to the generated synthetic function
/// Returns results that will be returned from the synthetic function
pub fn transform_miden_abi_call(
    import_func_ref: FunctionRef,
    import_func_id: FunctionIdent,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt,
) -> Vec<ValueRef> {
    use TransformStrategy::*;
    match get_transform_strategy(import_func_id.module.as_str(), import_func_id.function.as_str()) {
        ListReturn => list_return(import_func_ref, args, builder),
        ReturnViaPointer => return_via_pointer(import_func_ref, args, builder),
        NoTransform => no_transform(import_func_ref, args, builder),
    }
}

/// No transformation needed
#[inline(always)]
pub fn no_transform(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .ins()
        .exec(import_func_ref, signature, args.to_vec(), span)
        .expect("failed to build an exec op in no_transform strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    results
}

/// The Miden ABI function returns a length and a pointer and we only want the length
pub fn list_return(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .ins()
        .exec(import_func_ref, signature, args.to_vec(), span)
        .expect("failed to build an exec op in list_return strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    assert_eq!(results.len(), 2, "List return strategy expects 2 results: length and pointer");
    // Return the first result (length) only
    results[0..1].to_vec()
}

/// The Miden ABI function returns felts on the stack and we want to return via a pointer argument
pub fn return_via_pointer(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    // Omit the last argument (pointer)
    let args_wo_pointer = &args[0..args.len() - 1];
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .ins()
        .exec(import_func_ref, signature, args_wo_pointer.to_vec(), span)
        .expect("failed to build an exec op in return_via_pointer strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    let ptr_arg = *args.last().expect("empty args");
    let ptr_arg_ty = ptr_arg.borrow().ty().clone();
    assert_eq!(ptr_arg_ty, Type::I32);
    let ptr_u32 = builder.ins().bitcast(ptr_arg, Type::U32, span).expect("failed bitcast to U32");

    let result_ty =
        midenc_hir2::StructType::new(results.iter().map(|v| (*v).borrow().ty().clone()));
    for (idx, value) in results.iter().enumerate() {
        let value_ty = (*value).borrow().ty().clone().clone();
        let eff_ptr = if idx == 0 {
            // We're assuming here that the base pointer is of the correct alignment
            ptr_u32
        } else {
            let imm = Immediate::U32(result_ty.get(idx).offset);
            let imm_val = builder.ins().imm(imm, span);
            builder.ins().add(ptr_u32, imm_val, span).expect("failed add")
        };
        let addr = builder
            .ins()
            .inttoptr(eff_ptr, Type::Ptr(value_ty.into()), span)
            .expect("failed inttoptr");
        builder.ins().store(addr, *value, span).expect("failed store");
    }
    Vec::new()
}

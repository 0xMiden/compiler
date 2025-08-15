use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::FunctionRef,
    interner::{symbols, Symbol},
    Builder, Immediate, PointerType, SymbolNameComponent, SymbolPath, Type, ValueRef,
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
fn get_transform_strategy(path: &SymbolPath) -> Option<TransformStrategy> {
    let mut components = path.components().peekable();
    components.next_if_eq(&SymbolNameComponent::Root);

    match components.next()?.as_symbol_name() {
        symbols::Std => match components.next()?.as_symbol_name() {
            symbols::Mem => match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                stdlib::mem::PIPE_WORDS_TO_MEMORY | stdlib::mem::PIPE_DOUBLE_WORDS_TO_MEMORY => {
                    Some(TransformStrategy::ReturnViaPointer)
                }
                stdlib::mem::PIPE_PREIMAGE_TO_MEMORY => Some(TransformStrategy::NoTransform),
                _ => None,
            },
            symbols::Crypto => match components.next()?.as_symbol_name() {
                symbols::Hashes => match components.next()?.as_symbol_name() {
                    symbols::Blake3 => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::hashes::blake3::HASH_1TO1
                            | stdlib::crypto::hashes::blake3::HASH_2TO1 => {
                                Some(TransformStrategy::ReturnViaPointer)
                            }
                            _ => None,
                        }
                    }
                    name if name == Symbol::intern("rpo") => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::hashes::rpo::HASH_MEMORY => {
                                Some(TransformStrategy::ReturnViaPointer)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                },
                symbols::Dsa => match components.next()?.as_symbol_name() {
                    symbols::RpoFalcon => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::dsa::rpo_falcon::RPO_FALCON512_VERIFY => {
                                Some(TransformStrategy::NoTransform)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        },
        symbols::Miden => match components.next()?.as_symbol_name() {
            symbols::Account => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::account::INCR_NONCE => Some(TransformStrategy::NoTransform),
                    tx_kernel::account::ADD_ASSET
                    | tx_kernel::account::GET_ID
                    | tx_kernel::account::REMOVE_ASSET
                    | tx_kernel::account::GET_STORAGE_ITEM
                    | tx_kernel::account::SET_STORAGE_ITEM
                    | tx_kernel::account::GET_STORAGE_MAP_ITEM
                    | tx_kernel::account::SET_STORAGE_MAP_ITEM => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    _ => None,
                }
            }
            symbols::Note => match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                tx_kernel::note::GET_INPUTS => Some(TransformStrategy::ListReturn),
                tx_kernel::note::GET_ASSETS => Some(TransformStrategy::ListReturn),
                _ => None,
            },
            symbols::Tx => match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                tx_kernel::tx::CREATE_NOTE => Some(TransformStrategy::NoTransform),
                tx_kernel::tx::ADD_ASSET_TO_NOTE => Some(TransformStrategy::ReturnViaPointer),
                _ => None,
            },
            _ => None,
        },
        _ => None,
    }
}

/// Transform a Miden ABI function call based on the transformation strategy
///
/// `import_func` - import function that we're transforming a call to (think of a MASM function)
/// `args` - arguments to the generated synthetic function
/// Returns results that will be returned from the synthetic function
pub fn transform_miden_abi_call<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    import_path: &SymbolPath,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> Vec<ValueRef> {
    use TransformStrategy::*;
    match get_transform_strategy(import_path) {
        Some(ListReturn) => list_return(import_func_ref, args, builder),
        Some(ReturnViaPointer) => return_via_pointer(import_func_ref, args, builder),
        Some(NoTransform) => no_transform(import_func_ref, args, builder),
        None => panic!("no transform strategy implemented for '{import_path}'"),
    }
}

/// No transformation needed
#[inline(always)]
pub fn no_transform<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .exec(import_func_ref, signature, args.to_vec(), span)
        .expect("failed to build an exec op in no_transform strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    results
}

/// The Miden ABI function returns a length and a pointer and we only want the length
pub fn list_return<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
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
pub fn return_via_pointer<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> Vec<ValueRef> {
    let span = import_func_ref.borrow().name().span;
    // Omit the last argument (pointer)
    let args_wo_pointer = &args[0..args.len() - 1];
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .exec(import_func_ref, signature, args_wo_pointer.to_vec(), span)
        .expect("failed to build an exec op in return_via_pointer strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    let ptr_arg = *args.last().expect("empty args");
    let ptr_arg_ty = ptr_arg.borrow().ty().clone();
    assert_eq!(ptr_arg_ty, Type::I32);
    let ptr_u32 = builder.bitcast(ptr_arg, Type::U32, span).expect("failed bitcast to U32");

    let result_ty = midenc_hir::StructType::new(results.iter().map(|v| (*v).borrow().ty().clone()));
    for (idx, value) in results.iter().enumerate() {
        let value_ty = (*value).borrow().ty().clone().clone();
        let eff_ptr = if idx == 0 {
            // We're assuming here that the base pointer is of the correct alignment
            ptr_u32
        } else {
            let imm = Immediate::U32(result_ty.get(idx).offset);
            let imm_val = builder.imm(imm, span);
            builder.add(ptr_u32, imm_val, span).expect("failed add")
        };
        let addr = builder
            .inttoptr(eff_ptr, Type::from(PointerType::new(value_ty)), span)
            .expect("failed inttoptr");
        builder.store(addr, *value, span).expect("failed store");
    }
    Vec::new()
}

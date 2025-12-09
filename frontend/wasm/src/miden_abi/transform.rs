use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::FunctionRef,
    interner::symbols,
    Builder, Immediate, PointerType, SourceSpan, SymbolNameComponent, SymbolPath, Type, ValueRef,
};

use super::{stdlib, tx_kernel};
use crate::module::function_builder_ext::FunctionBuilderExt;

/// Returns a synthetic SourceSpan for compiler-generated code.
///
/// This uses SourceSpan::SYNTHETIC from miden-debug-types which is identified
/// by having an unknown source_id and both start and end set to u32::MAX.
/// This differentiates it from UNKNOWN spans (which have start and end at 0)
/// and indicates the code doesn't correspond to any specific user source location.
fn synthetic_span() -> SourceSpan {
    SourceSpan::SYNTHETIC
}

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
                    symbols::Sha256 => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::hashes::sha256::HASH_1TO1
                            | stdlib::crypto::hashes::sha256::HASH_2TO1 => {
                                Some(TransformStrategy::ReturnViaPointer)
                            }
                            _ => None,
                        }
                    }
                    symbols::Rpo => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::hashes::rpo::HASH_MEMORY
                            | stdlib::crypto::hashes::rpo::HASH_MEMORY_WORDS => {
                                Some(TransformStrategy::ReturnViaPointer)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                },
                symbols::Dsa => match components.next()?.as_symbol_name() {
                    symbols::RpoFalcon512 => {
                        match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                            stdlib::crypto::dsa::rpo_falcon512::RPO_FALCON512_VERIFY => {
                                Some(TransformStrategy::NoTransform)
                            }
                            _ => None,
                        }
                    }
                    _ => None,
                },
                _ => None,
            },
            symbols::Collections => {
                let submodule = components.next()?.as_symbol_name();
                if submodule == symbols::Smt {
                    return match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        stdlib::collections::smt::GET | stdlib::collections::smt::SET => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        _ => None,
                    };
                }
                None
            }
            _ => None,
        },
        symbols::Miden => match components.next()?.as_symbol_name() {
            symbols::NativeAccount => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::native_account::ADD_ASSET
                    | tx_kernel::native_account::REMOVE_ASSET
                    | tx_kernel::native_account::COMPUTE_DELTA_COMMITMENT
                    | tx_kernel::native_account::SET_STORAGE_ITEM
                    | tx_kernel::native_account::SET_STORAGE_MAP_ITEM => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    tx_kernel::native_account::INCR_NONCE
                    | tx_kernel::native_account::WAS_PROCEDURE_CALLED => {
                        Some(TransformStrategy::NoTransform)
                    }
                    _ => None,
                }
            }
            symbols::ActiveAccount => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::active_account::GET_NONCE
                    | tx_kernel::active_account::GET_BALANCE
                    | tx_kernel::active_account::GET_INITIAL_BALANCE
                    | tx_kernel::active_account::GET_NUM_PROCEDURES
                    | tx_kernel::active_account::HAS_NON_FUNGIBLE_ASSET
                    | tx_kernel::active_account::HAS_PROCEDURE => {
                        Some(TransformStrategy::NoTransform)
                    }
                    tx_kernel::active_account::GET_ID
                    | tx_kernel::active_account::GET_INITIAL_COMMITMENT
                    | tx_kernel::active_account::GET_CODE_COMMITMENT
                    | tx_kernel::active_account::COMPUTE_COMMITMENT
                    | tx_kernel::active_account::GET_INITIAL_STORAGE_COMMITMENT
                    | tx_kernel::active_account::COMPUTE_STORAGE_COMMITMENT
                    | tx_kernel::active_account::GET_STORAGE_ITEM
                    | tx_kernel::active_account::GET_INITIAL_STORAGE_ITEM
                    | tx_kernel::active_account::GET_STORAGE_MAP_ITEM
                    | tx_kernel::active_account::GET_INITIAL_STORAGE_MAP_ITEM
                    | tx_kernel::active_account::GET_INITIAL_VAULT_ROOT
                    | tx_kernel::active_account::GET_VAULT_ROOT
                    | tx_kernel::active_account::GET_PROCEDURE_ROOT => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    _ => None,
                }
            }
            symbols::Asset => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::asset::BUILD_FUNGIBLE_ASSET
                    | tx_kernel::asset::BUILD_NON_FUNGIBLE_ASSET => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    _ => None,
                }
            }
            symbols::Faucet => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::faucet::CREATE_FUNGIBLE_ASSET
                    | tx_kernel::faucet::CREATE_NON_FUNGIBLE_ASSET
                    | tx_kernel::faucet::MINT
                    | tx_kernel::faucet::BURN => Some(TransformStrategy::ReturnViaPointer),
                    tx_kernel::faucet::GET_TOTAL_ISSUANCE
                    | tx_kernel::faucet::IS_NON_FUNGIBLE_ASSET_ISSUED => {
                        Some(TransformStrategy::NoTransform)
                    }
                    _ => None,
                }
            }
            symbols::ActiveNote => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::active_note::GET_INPUTS => Some(TransformStrategy::ListReturn),
                    tx_kernel::active_note::GET_ASSETS => Some(TransformStrategy::ListReturn),
                    tx_kernel::active_note::GET_SENDER
                    | tx_kernel::active_note::GET_RECIPIENT
                    | tx_kernel::active_note::GET_SCRIPT_ROOT
                    | tx_kernel::active_note::GET_SERIAL_NUMBER
                    | tx_kernel::active_note::GET_METADATA => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    tx_kernel::active_note::ADD_ASSETS_TO_ACCOUNT => {
                        Some(TransformStrategy::NoTransform)
                    }
                    _ => None,
                }
            }
            symbols::InputNote => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::input_note::GET_ASSETS => Some(TransformStrategy::ListReturn),
                    tx_kernel::input_note::GET_ASSETS_INFO
                    | tx_kernel::input_note::GET_RECIPIENT
                    | tx_kernel::input_note::GET_METADATA
                    | tx_kernel::input_note::GET_SENDER
                    | tx_kernel::input_note::GET_INPUTS_INFO
                    | tx_kernel::input_note::GET_SCRIPT_ROOT
                    | tx_kernel::input_note::GET_SERIAL_NUMBER => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    _ => None,
                }
            }
            symbols::OutputNote => {
                match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                    tx_kernel::output_note::CREATE => Some(TransformStrategy::NoTransform),
                    tx_kernel::output_note::ADD_ASSET => Some(TransformStrategy::NoTransform),
                    tx_kernel::output_note::GET_ASSETS => Some(TransformStrategy::ListReturn),
                    tx_kernel::output_note::GET_ASSETS_INFO
                    | tx_kernel::output_note::GET_RECIPIENT
                    | tx_kernel::output_note::GET_METADATA => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    _ => None,
                }
            }
            symbols::Tx => match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                tx_kernel::tx::GET_BLOCK_NUMBER
                | tx_kernel::tx::GET_BLOCK_TIMESTAMP
                | tx_kernel::tx::GET_NUM_INPUT_NOTES
                | tx_kernel::tx::GET_NUM_OUTPUT_NOTES
                | tx_kernel::tx::GET_EXPIRATION_BLOCK_DELTA
                | tx_kernel::tx::UPDATE_EXPIRATION_BLOCK_DELTA => {
                    Some(TransformStrategy::NoTransform)
                }
                tx_kernel::tx::GET_INPUT_NOTES_COMMITMENT
                | tx_kernel::tx::GET_OUTPUT_NOTES_COMMITMENT
                | tx_kernel::tx::GET_BLOCK_COMMITMENT => Some(TransformStrategy::ReturnViaPointer),
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
    let exec_span = import_func_ref.borrow().name().span;
    // Omit the last argument (pointer)
    let args_wo_pointer = &args[0..args.len() - 1];
    let signature = import_func_ref.borrow().signature().clone();
    let exec = builder
        .exec(import_func_ref, signature, args_wo_pointer.to_vec(), exec_span)
        .expect("failed to build an exec op in return_via_pointer strategy");

    let borrow = exec.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    let ptr_arg = *args.last().expect("empty args");
    let ptr_arg_ty = ptr_arg.borrow().ty().clone();
    assert_eq!(ptr_arg_ty, Type::I32);
    // Use synthetic span for all compiler-generated ABI transformation operations
    // These operations are part of the return-via-pointer calling convention
    // and don't correspond to any specific user source code
    let span = synthetic_span();
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

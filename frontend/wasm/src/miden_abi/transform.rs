use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::{ExecFpi, HirOpBuilder};
use midenc_hir::{
    AddressSpace, Builder, Immediate, Op, PointerType, SourceSpan, SymbolNameComponent, SymbolPath,
    Type, ValueRef, dialects::builtin::FunctionRef, interner::symbols,
};
use midenc_session::diagnostics::Report;

use super::{stdlib, tx_kernel};
use crate::{
    error::WasmResult, fpi::store_fpi_prefix_locals,
    module::function_builder_ext::FunctionBuilderExt,
};

const RAW_FPI_FLATTENED_ARG_COUNT: u32 = ExecFpi::EXECUTOR_INPUT_FELTS as u32;
const RAW_FPI_FLATTENED_ARG_COUNT_USIZE: usize = ExecFpi::EXECUTOR_INPUT_FELTS;

/// The strategy to use for transforming a function call
enum TransformStrategy {
    /// The Miden ABI function returns a length and a pointer and we only want the length
    ListReturn,
    /// The Miden ABI function returns on the stack and we want to return via a pointer argument
    ReturnViaPointer,
    /// The import is the raw FPI binding, which passes the executor ABI through one pointer.
    FpiIndirectReturnViaPointer,
    /// No transformation needed
    NoTransform,
}

/// Get the transformation strategy for a function name
fn get_transform_strategy(path: &SymbolPath) -> Option<TransformStrategy> {
    let mut components = path.components().peekable();
    components.next_if_eq(&SymbolNameComponent::Root);

    match components.next()?.as_symbol_name() {
        symbols::Miden => match components.next()?.as_symbol_name() {
            symbols::Core => match components.next()?.as_symbol_name() {
                symbols::Mem => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        stdlib::mem::PIPE_WORDS_TO_MEMORY
                        | stdlib::mem::PIPE_DOUBLE_WORDS_TO_MEMORY => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        stdlib::mem::PIPE_PREIMAGE_TO_MEMORY => {
                            Some(TransformStrategy::NoTransform)
                        }
                        _ => None,
                    }
                }
                symbols::Crypto => match components.next()?.as_symbol_name() {
                    symbols::Hashes => match components.next()?.as_symbol_name() {
                        symbols::Blake3 => {
                            match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                                stdlib::crypto::hashes::blake3::HASH
                                | stdlib::crypto::hashes::blake3::MERGE => {
                                    Some(TransformStrategy::ReturnViaPointer)
                                }
                                _ => None,
                            }
                        }
                        symbols::Sha256 => {
                            match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                                stdlib::crypto::hashes::sha256::HASH
                                | stdlib::crypto::hashes::sha256::MERGE => {
                                    Some(TransformStrategy::ReturnViaPointer)
                                }
                                _ => None,
                            }
                        }
                        symbols::Poseidon2 => {
                            match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                                stdlib::crypto::hashes::poseidon2::HASH_ELEMENTS
                                | stdlib::crypto::hashes::poseidon2::HASH_WORDS
                                | stdlib::crypto::hashes::poseidon2::MERGE => {
                                    Some(TransformStrategy::ReturnViaPointer)
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    },
                    symbols::Dsa => match components.next()?.as_symbol_name() {
                        symbols::Falcon512Poseidon2 => {
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
                        return match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str()
                        {
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
            symbols::Protocol => match components.next()?.as_symbol_name() {
                symbols::NativeAccount => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::native_account::ADD_ASSET
                        | tx_kernel::native_account::REMOVE_ASSET
                        | tx_kernel::native_account::GET_ID
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
                module if module == symbols::Note => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::note::COMPUTE_AND_STORE_RECIPIENT => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        tx_kernel::note::WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY
                        | tx_kernel::note::WRITE_ATTACHMENT_TO_MEMORY
                        | tx_kernel::note::WRITE_INDEXED_ATTACHMENT_TO_MEMORY => {
                            Some(TransformStrategy::ListReturn)
                        }
                        tx_kernel::note::COMPUTE_STORAGE_COMMITMENT
                        | tx_kernel::note::COMPUTE_RECIPIENT
                        | tx_kernel::note::METADATA_INTO_SENDER
                        | tx_kernel::note::METADATA_INTO_ATTACHMENT_SCHEMES
                        | tx_kernel::note::FIND_ATTACHMENT_IDX => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        tx_kernel::note::METADATA_INTO_NOTE_TYPE
                        | tx_kernel::note::METADATA_INTO_TAG => {
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
                        | tx_kernel::active_account::GET_ASSET
                        | tx_kernel::active_account::GET_INITIAL_ASSET
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
                        tx_kernel::asset::CREATE_FUNGIBLE_ASSET
                        | tx_kernel::asset::CREATE_NON_FUNGIBLE_ASSET => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        _ => None,
                    }
                }
                symbols::Faucet => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::faucet::CREATE_FUNGIBLE_ASSET
                        | tx_kernel::faucet::CREATE_NON_FUNGIBLE_ASSET => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        tx_kernel::faucet::MINT
                        | tx_kernel::faucet::BURN
                        | tx_kernel::faucet::HAS_CALLBACKS => Some(TransformStrategy::NoTransform),
                        _ => None,
                    }
                }
                symbols::ActiveNote => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::active_note::GET_STORAGE
                        | tx_kernel::active_note::GET_ASSETS
                        | tx_kernel::active_note::WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY
                        | tx_kernel::active_note::WRITE_ATTACHMENT_TO_MEMORY => {
                            Some(TransformStrategy::ListReturn)
                        }
                        tx_kernel::active_note::IS_PUBLIC | tx_kernel::active_note::IS_PRIVATE => {
                            Some(TransformStrategy::NoTransform)
                        }
                        tx_kernel::active_note::GET_SENDER
                        | tx_kernel::active_note::GET_RECIPIENT
                        | tx_kernel::active_note::GET_SCRIPT_ROOT
                        | tx_kernel::active_note::GET_SERIAL_NUMBER
                        | tx_kernel::active_note::GET_METADATA
                        | tx_kernel::active_note::GET_ATTACHMENTS_COMMITMENT
                        | tx_kernel::active_note::FIND_ATTACHMENT => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        _ => None,
                    }
                }
                symbols::InputNote => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::input_note::GET_ASSETS
                        | tx_kernel::input_note::WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY
                        | tx_kernel::input_note::WRITE_ATTACHMENT_TO_MEMORY => {
                            Some(TransformStrategy::ListReturn)
                        }
                        tx_kernel::input_note::GET_ASSETS_INFO
                        | tx_kernel::input_note::GET_RECIPIENT
                        | tx_kernel::input_note::GET_METADATA
                        | tx_kernel::input_note::GET_SENDER
                        | tx_kernel::input_note::GET_STORAGE_INFO
                        | tx_kernel::input_note::GET_SCRIPT_ROOT
                        | tx_kernel::input_note::GET_SERIAL_NUMBER
                        | tx_kernel::input_note::GET_ATTACHMENTS_COMMITMENT
                        | tx_kernel::input_note::GET_ATTACHMENTS_COMMITMENT_RAW
                        | tx_kernel::input_note::FIND_ATTACHMENT => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        _ => None,
                    }
                }
                symbols::OutputNote => {
                    match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str() {
                        tx_kernel::output_note::CREATE => Some(TransformStrategy::NoTransform),
                        tx_kernel::output_note::ADD_ASSET => Some(TransformStrategy::NoTransform),
                        tx_kernel::output_note::ADD_ATTACHMENT
                        | tx_kernel::output_note::ADD_WORD_ATTACHMENT
                        | tx_kernel::output_note::ADD_ATTACHMENT_FROM_MEMORY => {
                            Some(TransformStrategy::NoTransform)
                        }
                        tx_kernel::output_note::GET_ASSETS
                        | tx_kernel::output_note::WRITE_ATTACHMENT_COMMITMENTS_TO_MEMORY
                        | tx_kernel::output_note::WRITE_ATTACHMENT_TO_MEMORY => {
                            Some(TransformStrategy::ListReturn)
                        }
                        tx_kernel::output_note::GET_ASSETS_INFO
                        | tx_kernel::output_note::GET_RECIPIENT
                        | tx_kernel::output_note::GET_METADATA
                        | tx_kernel::output_note::GET_ATTACHMENTS_COMMITMENT
                        | tx_kernel::output_note::FIND_ATTACHMENT => {
                            Some(TransformStrategy::ReturnViaPointer)
                        }
                        _ => None,
                    }
                }
                symbols::Tx => match components.next_if(|c| c.is_leaf())?.as_symbol_name().as_str()
                {
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
                    | tx_kernel::tx::GET_BLOCK_COMMITMENT
                    | tx_kernel::tx::GET_TX_SCRIPT_ROOT => {
                        Some(TransformStrategy::ReturnViaPointer)
                    }
                    tx_kernel::tx::EXECUTE_FOREIGN_PROCEDURE_INDIRECT => {
                        Some(TransformStrategy::FpiIndirectReturnViaPointer)
                    }
                    _ => None,
                },
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
) -> WasmResult<Vec<ValueRef>> {
    use TransformStrategy::*;
    match get_transform_strategy(import_path) {
        Some(ListReturn) => list_return(import_func_ref, args, builder),
        Some(ReturnViaPointer) => return_via_pointer(import_func_ref, args, builder),
        Some(FpiIndirectReturnViaPointer) => {
            fpi_indirect_return_via_pointer(import_func_ref, args, builder)
        }
        Some(NoTransform) => no_transform(import_func_ref, args, builder),
        None => Err(Report::msg(format!("no transform strategy implemented for '{import_path}'"))),
    }
}

/// No transformation needed
#[inline(always)]
pub fn no_transform<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> WasmResult<Vec<ValueRef>> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().get_signature().clone();
    let exec = builder.exec(import_func_ref, signature, args.to_vec(), span)?;

    let borrow = exec.borrow();
    let results_storage = borrow.results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    Ok(results)
}

/// The Miden ABI function returns a length and a pointer and we only want the length
pub fn list_return<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> WasmResult<Vec<ValueRef>> {
    let span = import_func_ref.borrow().name().span;
    let signature = import_func_ref.borrow().get_signature().clone();
    let exec = builder.exec(import_func_ref, signature, args.to_vec(), span)?;

    let borrow = exec.borrow();
    let results_storage = borrow.results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    if results.len() != 2 {
        return Err(Report::msg(format!(
            "list return strategy expects 2 results, length and pointer, but received {}",
            results.len()
        )));
    }

    Ok(results[0..1].to_vec())
}

/// The Miden ABI function returns felts on the stack and we want to return via a pointer argument
pub fn return_via_pointer<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> WasmResult<Vec<ValueRef>> {
    let span = import_func_ref.borrow().name().span;
    let Some((ptr_arg, args_wo_pointer)) = args.split_last() else {
        return Err(Report::msg(
            "return-via-pointer strategy expects a trailing output pointer argument",
        ));
    };
    let signature = import_func_ref.borrow().get_signature().clone();
    let exec = builder.exec(import_func_ref, signature, args_wo_pointer.to_vec(), span)?;

    let borrow = exec.borrow();
    let results_storage = borrow.results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    store_results_to_pointer(&results, *ptr_arg, builder)?;

    Ok(Vec::new())
}

/// The raw FPI executor passes the full felt-only executor ABI through one invocation pointer.
///
/// The generated stub reloads the 22 invocation felts from memory, stores the 6-felt prefix into
/// function locals, and emits the same `hir.exec_fpi` form used by typed FPI imports, so raw FPI
/// calls reach the backend with no special shape.
pub fn fpi_indirect_return_via_pointer<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    args: &[ValueRef],
    builder: &mut FunctionBuilderExt<'_, B>,
) -> WasmResult<Vec<ValueRef>> {
    let span = import_func_ref.borrow().name().span;
    let Some((ptr_arg, args_wo_pointer)) = args.split_last() else {
        return Err(Report::msg(
            "indirect FPI return strategy expects one input tuple pointer and one output pointer",
        ));
    };
    let [invocation_ptr] = *args_wo_pointer else {
        return Err(Report::msg(format!(
            "indirect FPI return strategy expects exactly one input tuple pointer before the \
             output pointer, but received {} input operands",
            args_wo_pointer.len()
        )));
    };

    // The Rust binding stores the invocation as 22 consecutive felts: account id prefix, account
    // id suffix, the procedure root word, and the 16 padded procedure input felts.
    let felt_ptr_ty =
        Type::from(PointerType::new_with_address_space(Type::Felt, AddressSpace::Byte));
    let mut fpi_args = Vec::with_capacity(RAW_FPI_FLATTENED_ARG_COUNT_USIZE);
    for index in 0..RAW_FPI_FLATTENED_ARG_COUNT {
        let addr = if index == 0 {
            invocation_ptr
        } else {
            let byte_offset = builder.i32((index * 4) as i32, span);
            builder.add_unchecked(invocation_ptr, byte_offset, span)?
        };
        let typed_ptr = builder.inttoptr(addr, felt_ptr_ty.clone(), span)?;
        fpi_args.push(builder.load(typed_ptr, span)?);
    }

    let prefix_locals = store_fpi_prefix_locals(builder, &fpi_args[..ExecFpi::PREFIX_FELTS], span)?;
    let procedure_inputs = fpi_args[ExecFpi::PREFIX_FELTS..].iter().copied();
    let exec = builder.exec_fpi(prefix_locals, procedure_inputs, span)?;

    let borrow = exec.borrow();
    let results_storage = borrow.results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();

    store_results_to_pointer(&results, *ptr_arg, builder)?;

    Ok(Vec::new())
}

/// Stores flattened stack results into the Rust return pointer used by linker stubs.
pub(crate) fn store_results_to_pointer<B: ?Sized + Builder>(
    results: &[ValueRef],
    ptr_arg: ValueRef,
    builder: &mut FunctionBuilderExt<'_, B>,
) -> WasmResult<()> {
    // Use synthetic span for all compiler-generated ABI transformation operations
    // These operations are part of the return-via-pointer calling convention
    // and don't correspond to any specific user source code
    let span = SourceSpan::SYNTHETIC;
    let ptr_arg_ty = ptr_arg.borrow().ty().clone();
    if ptr_arg_ty != Type::I32 {
        return Err(Report::msg(format!(
            "return-via-pointer strategy expects an `i32` output pointer argument, but received \
             `{ptr_arg_ty}`"
        )));
    }

    let ptr_u32 = builder.bitcast(ptr_arg, Type::U32, span)?;

    let result_ty = midenc_hir::StructType::new(results.iter().map(|v| (*v).borrow().ty().clone()));
    for (idx, value) in results.iter().enumerate() {
        let value_ty = (*value).borrow().ty().clone().clone();
        let eff_ptr = if idx == 0 {
            // We're assuming here that the base pointer is of the correct alignment
            ptr_u32
        } else {
            let imm = Immediate::U32(result_ty.get(idx).offset);
            let imm_val = builder.imm(imm, span);
            builder.add(ptr_u32, imm_val, span)?
        };
        let addr = builder.inttoptr(eff_ptr, Type::from(PointerType::new(value_ty)), span)?;
        builder.store(addr, *value, span)?;
    }

    Ok(())
}

//! Generic lowering for Rust linker stubs to MASM procedure calls.
//! A linker stub is detected as a function whose body consists solely of a
//! single `unreachable` instruction (plus the implicit `end`). The stub
//! function name is expected to be a fully-qualified MASM function path like
//! `miden::account::add_asset` and is used to locate the MASM callee.

use core::str::FromStr;
use std::{cell::RefCell, rc::Rc};

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{BuiltinOpBuilder, FunctionRef, ModuleBuilder},
    AbiParam, Context, FunctionType, Signature, SmallVec, SymbolPath, ValueRef,
};
use wasmparser::{FunctionBody, Operator};

use crate::{
    intrinsics::{convert_intrinsics_call, Intrinsic},
    miden_abi::{
        is_miden_abi_module, miden_abi_function_type,
        transform::transform_miden_abi_call,
    },
    module::{
        function_builder_ext::{FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener},
        module_translation_state::ModuleTranslationState,
    },
};

/// Returns true if the given Wasm function body consists only of an
/// `unreachable` operator (ignoring `end`/`nop`).
pub fn is_unreachable_stub(body: &FunctionBody<'_>) -> bool {
    let mut reader = match body.get_operators_reader() {
        Ok(r) => r,
        Err(_) => return false,
    };
    while !reader.eof() {
        let Ok((op, _)) = reader.read_with_offset() else {
            return false;
        };
        match op {
            Operator::Unreachable => {
                // If we see more than one meaningful operator, still ok: allow repeated unreachable
                return true;
            }
            Operator::End | Operator::Nop => {
                // ignore
            }
            _ => return false,
        }
    }
    false
}

/// If `body` looks like a linker stub, lowers `function_ref` to a call to the
/// MASM callee derived from the function name and applies the appropriate
/// TransformStrategy. Returns `true` if handled, `false` otherwise.
pub fn maybe_lower_linker_stub(
    function_ref: FunctionRef,
    body: &FunctionBody<'_>,
    module_state: &mut ModuleTranslationState,
    context: Rc<Context>,
) -> Result<bool, midenc_hir::Report> {
    if !is_unreachable_stub(body) {
        return Ok(false);
    }

    // Parse function name as MASM function ident: "ns::...::func"
    let name_string = {
        let borrowed = function_ref.borrow();
        borrowed.name().as_str().to_string()
    };
    let func_ident = match midenc_hir::FunctionIdent::from_str(&name_string) {
        Ok(id) => id,
        Err(_) => return Ok(false),
    };
    let import_path: SymbolPath = SymbolPath::from_masm_function_id(func_ident);
    // Ensure the stub targets a known Miden ABI module or a recognized intrinsic.
    let is_intrinsic = Intrinsic::try_from(&import_path).is_ok();
    if !is_miden_abi_module(&import_path) && !is_intrinsic {
        return Ok(false);
    }

    // Classify intrinsics and obtain signature when needed
    let (import_sig, intrinsic): (Signature, Option<Intrinsic>) = match Intrinsic::try_from(&import_path) {
        Ok(intr) => (function_ref.borrow().signature().clone(), Some(intr)),
        Err(_) => {
            let import_ft: FunctionType = miden_abi_function_type(&import_path);
            (
                Signature::new(
                    import_ft.params.into_iter().map(AbiParam::new),
                    import_ft.results.into_iter().map(AbiParam::new),
                ),
                None,
            )
        }
    };

    // Build the function body for the stub and replace it with an exec to MASM
    let span = function_ref.borrow().name().span;
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder = midenc_hir::OpBuilder::new(context.clone())
        .with_listener(SSABuilderListener::new(func_ctx));
    let mut fb = FunctionBuilderExt::new(function_ref, &mut op_builder);

    // Entry/args
    let entry_block = fb.current_block();
    fb.seal_block(entry_block);
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    // Declare MASM import callee in world and exec via TransformStrategy
    let results: Vec<ValueRef> = if let Some(intr) = intrinsic {
        // Decide whether the intrinsic is implemented as a function or an operation
        let conv = intr
            .conversion_result()
            .expect("unknown intrinsic");
        if conv.is_function() {
            // Declare callee and call via convert_intrinsics_call with function_ref
            let import_module_ref = module_state
                .world_builder
                .declare_module_tree(&import_path.without_leaf())
                .wrap_err("failed to create module for intrinsics imports")?;
            let mut import_module_builder = ModuleBuilder::new(import_module_ref);
            let intrinsic_func_ref = import_module_builder
                .define_function(import_path.name().into(), import_sig.clone())
                .expect("failed to create intrinsic function ref");
            convert_intrinsics_call(intr, Some(intrinsic_func_ref), &args, &mut fb, span)
                .expect("convert_intrinsics_call failed")
                .to_vec()
        } else {
            // Inline conversion of intrinsic operation
            convert_intrinsics_call(intr, None, &args, &mut fb, span)
                .expect("convert_intrinsics_call failed")
                .to_vec()
        }
    } else {
        // Miden ABI path: exec import with TransformStrategy
        let import_module_ref = module_state
            .world_builder
            .declare_module_tree(&import_path.without_leaf())
            .wrap_err("failed to create module for MASM imports")?;
        let mut import_module_builder = ModuleBuilder::new(import_module_ref);
        let import_func_ref = import_module_builder
            .define_function(import_path.name().into(), import_sig)
            .expect("failed to create MASM import function ref");
        transform_miden_abi_call(import_func_ref, &import_path, &args, &mut fb)
    };

    // Return
    let exit_block = fb.create_block();
    fb.append_block_params_for_function_returns(exit_block);
    fb.br(exit_block, results, span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let ret_vals: SmallVec<[ValueRef; 1]> = {
        let borrow = exit_block.borrow();
        borrow.argument_values().collect()
    };
    fb.ret(ret_vals, span)?;

    Ok(true)
}

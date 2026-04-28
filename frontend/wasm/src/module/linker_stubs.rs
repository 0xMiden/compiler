//! Generic lowering for Rust linker stubs to MASM procedure calls.
//!
//! A linker stub is detected by its fully-qualified MASM function path, such as
//! `miden::native_account::add_asset`. Older stubs also have a body consisting solely of a single
//! `unreachable` instruction, but LTO can optimize callers incorrectly when those bodies are
//! visible, so current stubs use opaque returning bodies instead.

use alloc::rc::Rc;
use core::{cell::RefCell, str::FromStr};

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    FunctionIdent, FunctionType, Op, SmallVec, SymbolPath, ValueRef, Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{BuiltinOpBuilder, FunctionRef, ModuleBuilder, attributes::Signature},
};
use midenc_hir_symbol::symbols;
use wasmparser::{FunctionBody, Operator};

use crate::{
    error::WasmResult,
    intrinsics::{Intrinsic, IntrinsicsConversionResult, convert_intrinsics_call},
    miden_abi::{
        is_miden_abi_module, miden_abi_function_type, transform::transform_miden_abi_call,
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
    let mut saw_unreachable = false;
    while !reader.eof() {
        let Ok((op, _)) = reader.read_with_offset() else {
            return false;
        };
        match op {
            Operator::Unreachable => {
                saw_unreachable = true;
            }
            Operator::End | Operator::Nop => {
                // ignore
            }
            _ => return false,
        }
    }
    saw_unreachable
}

/// Returns the Miden ABI or intrinsic import path encoded in a linker stub function name.
pub fn linker_stub_import_path(function_name: &str) -> Option<SymbolPath> {
    let func_ident = FunctionIdent::from_str(function_name).ok()?;
    let import_path = SymbolPath::from_masm_function_id(func_ident);
    let is_intrinsic = Intrinsic::try_from(&import_path).is_ok();
    if is_miden_abi_module(&import_path) || is_intrinsic {
        Some(import_path)
    } else {
        None
    }
}

/// Returns true when `signature` matches the given canonical Wasm function type.
pub fn signature_matches_function_type(
    signature: &Signature,
    function_type: &FunctionType,
) -> bool {
    signature.params().len() == function_type.params.len()
        && signature.results().len() == function_type.results.len()
        && signature
            .params()
            .iter()
            .zip(&function_type.params)
            .all(|(actual, expected)| actual.ty == *expected)
        && signature
            .results()
            .iter()
            .zip(&function_type.results)
            .all(|(actual, expected)| actual.ty == *expected)
}

/// If `body` looks like a linker stub, lowers `function_ref` to a call to the
/// MASM callee derived from the function name and applies the appropriate
/// TransformStrategy. Returns `true` if handled, `false` otherwise.
pub fn maybe_lower_linker_stub(
    function_ref: FunctionRef,
    body: &FunctionBody<'_>,
    module_state: &mut ModuleTranslationState,
) -> WasmResult<bool> {
    // Parse function name as MASM function ident: "ns::...::func"
    let name_string = {
        let borrowed = function_ref.borrow();
        borrowed.name().as_str().to_string()
    };
    let Some(import_path) = linker_stub_import_path(&name_string) else {
        if is_unreachable_stub(body)
            && let Ok(func_ident) = FunctionIdent::from_str(&name_string)
        {
            let import_path = SymbolPath::from_masm_function_id(func_ident);
            if import_path.namespace() == Some(symbols::Miden) {
                panic!(
                    "Failed to recognize miden stub: {}, check that symbols.toml (used to \
                     generate`symbols::<Symbol>` values) has all the parts right and it's \
                     signature is defined in the frontend/wasm/src/miden_abi/",
                    import_path.to_library_path()
                );
            }
        }
        return Ok(false);
    };
    let is_intrinsic = Intrinsic::try_from(&import_path).is_ok();
    if !is_miden_abi_module(&import_path) && !is_intrinsic {
        if import_path.namespace() == Some(symbols::Miden) {
            panic!(
                "Failed to recognize miden stub: {}, check that symbols.toml (used to \
                 generate`symbols::<Symbol>` values) has all the parts right and it's signature \
                 is defined in the frontend/wasm/src/miden_abi/",
                import_path.to_library_path()
            );
        }
        return Ok(false);
    }

    let context = function_ref.borrow().as_operation().context_rc();

    // Classify intrinsics and obtain signature when needed
    let (import_sig, intrinsic): (Signature, Option<Intrinsic>) =
        match Intrinsic::try_from(&import_path) {
            Ok(intr) => (function_ref.borrow().get_signature().clone(), Some(intr)),
            Err(_) => {
                let import_ft: FunctionType = miden_abi_function_type(&import_path);
                (Signature::new(&context, import_ft.params, import_ft.results), None)
            }
        };

    // Build the function body for the stub and replace it with an exec to MASM
    let span = function_ref.borrow().name().span;
    let func_builder_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder = midenc_hir::OpBuilder::new(context)
        .with_listener(SSABuilderListener::new(func_builder_ctx));
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
    let mut results: Vec<ValueRef> = if let Some(intr) = intrinsic {
        // Decide whether the intrinsic is implemented as a function or an operation
        let Some(conv) = intr.conversion_result() else {
            return Ok(false);
        };
        if conv.is_function() {
            // Declare callee and call via convert_intrinsics_call with function_ref
            let import_module_ref = module_state
                .world_builder
                .declare_module_tree(&import_path.without_leaf())
                .wrap_err("failed to create module for intrinsics imports")?;
            let mut import_module_builder = ModuleBuilder::new(import_module_ref);
            let intrinsic_func_ref = import_module_builder
                .define_function(import_path.name().into(), Visibility::Public, import_sig.clone())
                .wrap_err("failed to create intrinsic function ref")?;
            convert_intrinsics_call(intr, Some(intrinsic_func_ref), &args, &mut fb, span)?.to_vec()
        } else {
            let IntrinsicsConversionResult::MidenVmOp(function_type) = conv else {
                unreachable!("function conversions are handled above");
            };
            if !signature_matches_function_type(
                &function_ref.borrow().get_signature(),
                &function_type,
            ) {
                return Ok(false);
            }
            // Inline conversion of intrinsic operation
            convert_intrinsics_call(intr, None, &args, &mut fb, span)?.to_vec()
        }
    } else {
        // Miden ABI path: exec import with TransformStrategy
        let import_module_ref = module_state
            .world_builder
            .declare_module_tree(&import_path.without_leaf())
            .wrap_err("failed to create module for MASM imports")?;
        let mut import_module_builder = ModuleBuilder::new(import_module_ref);
        let import_func_ref = import_module_builder
            .define_function(import_path.name().into(), Visibility::Public, import_sig)
            .wrap_err("failed to create MASM import function ref")?;
        transform_miden_abi_call(import_func_ref, &import_path, &args, &mut fb)
    };
    let expected_results = function_ref.borrow().get_signature().results().len();
    truncate_dead_lto_results(&name_string, &mut results, expected_results);

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

/// Truncates ABI results that LTO proved unused in the Wasm wrapper signature.
///
/// Linker stubs are compiled into the final Wasm module and can be optimized together with their
/// callers under LTO. If every caller ignores a return value, LLVM can rewrite the local Wasm stub
/// to return fewer values than the underlying MASM ABI procedure. The synthesized HIR function must
/// follow the actual Wasm signature, so extra ABI results are left unused and dropped by later
/// lowering.
fn truncate_dead_lto_results(function_name: &str, results: &mut Vec<ValueRef>, expected: usize) {
    if results.len() >= expected {
        results.truncate(expected);
    } else {
        panic!(
            "linker stub '{function_name}' produced {} result(s), but its Wasm signature expects \
             {expected}",
            results.len()
        );
    }
}

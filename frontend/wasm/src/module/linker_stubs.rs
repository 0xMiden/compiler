//! Generic lowering for Rust linker stubs to MASM procedure calls.
//!
//! A linker stub is detected by its fully-qualified MASM function path, such as
//! `miden::native_account::add_asset`. Older stubs also have a body consisting solely of a single
//! `unreachable` instruction, but LTO can optimize callers incorrectly when those bodies are
//! visible, so current stubs use opaque returning bodies instead.

use alloc::rc::Rc;
use core::{cell::RefCell, str::FromStr};

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_hir::{
    FunctionIdent, FunctionType, Op, SmallVec, SymbolPath, ValueRef, Visibility,
    diagnostics::{Report, WrapErr},
    dialects::builtin::{BuiltinOpBuilder, FunctionRef, ModuleBuilder, attributes::Signature},
};
use midenc_hir_symbol::symbols;
use wasmparser::{FunctionBody, Operator};

use crate::{
    error::WasmResult,
    intrinsics::{Intrinsic, IntrinsicsConversionResult, convert_intrinsics_call},
    miden_abi::{
        is_miden_abi_module, transform::transform_miden_abi_call, try_miden_abi_function_type,
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

/// Returns true when the parsed linker stub signature is compatible with the canonical Wasm
/// function type.
///
/// LTO may trim trailing results from a Rust linker stub when all callers ignore them. Parameters
/// must still match exactly, and any retained results must match the leading canonical results.
pub fn stub_signature_matches_function_type(
    signature: &Signature,
    function_type: &FunctionType,
) -> bool {
    signature.params().len() == function_type.params.len()
        && stub_signature_results_match_function_type(signature, function_type)
        && signature
            .params()
            .iter()
            .zip(&function_type.params)
            .all(|(actual, expected)| actual.ty == *expected)
}

/// Returns true when retained Wasm stub results match a canonical result prefix.
fn stub_signature_results_match_function_type(
    signature: &Signature,
    function_type: &FunctionType,
) -> bool {
    signature.results().len() <= function_type.results.len()
        && signature
            .results()
            .iter()
            .zip(function_type.results.iter())
            .all(|(actual, expected)| actual.ty == *expected)
}

/// If `body` looks like a linker stub, lowers `function_ref` to a call to the
/// MASM callee derived from the function name and applies the appropriate
/// TransformStrategy. Returns `true` if handled, `false` otherwise.
pub fn maybe_lower_linker_stub(
    function_ref: FunctionRef,
    stub_signature: &Signature,
    body: &FunctionBody<'_>,
    module_state: &mut ModuleTranslationState,
    specialized_felt_from_u64_arg: Option<i64>,
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

    let intrinsic = Intrinsic::try_from(&import_path).ok();

    // Build the function body for the stub and replace it with an exec to MASM
    let span = function_ref.borrow().name().span;
    let func_builder_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder = midenc_hir::OpBuilder::new(context.clone())
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
    let results: Vec<ValueRef> = if let Some(intr) = intrinsic {
        // Decide whether the intrinsic is implemented as a function or an operation
        let conv = require_intrinsic_conversion_result(&name_string, &import_path, intr)?;
        match conv {
            IntrinsicsConversionResult::FunctionType(function_type) => {
                require_stub_signature_matches_function_type(
                    &name_string,
                    stub_signature,
                    &function_type,
                )?;

                let import_sig =
                    Signature::new(&context, function_type.params, function_type.results);
                // Declare callee and call via convert_intrinsics_call with function_ref
                let import_module_ref = module_state
                    .world_builder
                    .declare_module_tree(&import_path.without_leaf())
                    .wrap_err("failed to create module for intrinsics imports")?;
                let mut import_module_builder = ModuleBuilder::new(import_module_ref);
                let intrinsic_func_ref = import_module_builder
                    .define_function(
                        import_path.name().into(),
                        Visibility::Public,
                        import_sig.clone(),
                    )
                    .wrap_err("failed to create intrinsic function ref")?;
                convert_intrinsics_call(intr, Some(intrinsic_func_ref), &args, &mut fb, span)?
                    .to_vec()
            }
            IntrinsicsConversionResult::MidenVmOp(function_type) => {
                let args = if stub_signature_matches_function_type(stub_signature, &function_type) {
                    args
                } else if let Some(value) = specialized_felt_from_u64_arg
                    && is_felt_from_u64_unchecked(intr)
                    && stub_signature.params().is_empty()
                    && stub_signature_results_match_function_type(stub_signature, &function_type)
                {
                    vec![fb.i64(value, span)]
                } else {
                    require_stub_signature_matches_function_type(
                        &name_string,
                        stub_signature,
                        &function_type,
                    )?;
                    unreachable!("signature validation returned Ok for an incompatible stub")
                };

                convert_intrinsics_call(intr, None, &args, &mut fb, span)?.to_vec()
            }
        }
    } else {
        // Miden ABI path: exec import with TransformStrategy
        let import_module_ref = module_state
            .world_builder
            .declare_module_tree(&import_path.without_leaf())
            .wrap_err("failed to create module for MASM imports")?;
        let mut import_module_builder = ModuleBuilder::new(import_module_ref);
        let import_ft = require_miden_abi_function_type(&name_string, &import_path)?;
        let import_sig = Signature::new(&context, import_ft.params, import_ft.results);
        let import_func_ref = import_module_builder
            .define_function(import_path.name().into(), Visibility::Public, import_sig)
            .wrap_err("failed to create MASM import function ref")?;
        transform_miden_abi_call(import_func_ref, &import_path, &args, &mut fb)
    };
    let results = retain_stub_signature_results(&name_string, results, stub_signature);

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

/// Builds a diagnostic for a recognized linker stub that cannot be lowered.
fn recognized_stub_error(function_name: &str, reason: impl core::fmt::Display) -> Report {
    Report::msg(format!(
        "failed to lower recognized Miden linker stub '{function_name}': {reason}"
    ))
}

/// Returns true for the only intrinsic op stub we can lower after LTO removes its parameter.
fn is_felt_from_u64_unchecked(intrinsic: Intrinsic) -> bool {
    matches!(intrinsic, Intrinsic::Felt(function) if function.as_str() == "from_u64_unchecked")
}

/// Returns the registered lowering for a recognized intrinsic, or a hard lowering error.
fn require_intrinsic_conversion_result(
    function_name: &str,
    import_path: &SymbolPath,
    intrinsic: Intrinsic,
) -> WasmResult<IntrinsicsConversionResult> {
    intrinsic.conversion_result().ok_or_else(|| {
        recognized_stub_error(
            function_name,
            format!("intrinsic '{import_path}' is recognized, but has no registered lowering"),
        )
    })
}

/// Validates a recognized linker stub against its canonical Wasm function type.
fn require_stub_signature_matches_function_type(
    function_name: &str,
    signature: &Signature,
    function_type: &FunctionType,
) -> WasmResult<()> {
    if stub_signature_matches_function_type(signature, function_type) {
        return Ok(());
    }

    Err(recognized_stub_error(
        function_name,
        format!(
            "stub signature params={:?}, results={:?} is incompatible with canonical params={:?}, \
             results={:?}",
            signature.params().iter().map(|param| param.ty.clone()).collect::<Vec<_>>(),
            signature.results().iter().map(|result| result.ty.clone()).collect::<Vec<_>>(),
            function_type.params,
            function_type.results
        ),
    ))
}

/// Returns the registered Miden ABI signature for a recognized linker stub path.
fn require_miden_abi_function_type(
    function_name: &str,
    import_path: &SymbolPath,
) -> WasmResult<FunctionType> {
    try_miden_abi_function_type(import_path).ok_or_else(|| {
        recognized_stub_error(
            function_name,
            format!("Miden ABI path '{import_path}' is recognized, but has no signature entry"),
        )
    })
}

/// Retains only the results required by the parsed Wasm stub signature.
///
/// Linker stubs are compiled into the final Wasm module and can be optimized together with their
/// callers under LTO. If every caller ignores a return value, LLVM can rewrite the local Wasm stub
/// to return fewer values than the underlying MASM ABI procedure. The expected arity comes from
/// the core Wasm stub signature rather than the synthesized HIR function body.
fn retain_stub_signature_results(
    function_name: &str,
    mut results: Vec<ValueRef>,
    stub_signature: &Signature,
) -> Vec<ValueRef> {
    let expected = stub_signature.results().len();
    if results.len() < expected {
        panic!(
            "linker stub '{function_name}' produced {} result(s), but its Wasm signature expects \
             {expected}",
            results.len()
        );
    }

    results.truncate(expected);
    results
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{
        CallConv, Context, SymbolNameComponent, Type,
        interner::{Symbol, symbols},
    };

    use super::*;
    use crate::intrinsics::mem::HEAP_BASE;

    fn signature(
        params: impl IntoIterator<Item = Type>,
        results: impl IntoIterator<Item = Type>,
    ) -> Signature {
        Signature::new(&Rc::new(Context::default()), params, results)
    }

    #[test]
    fn stub_signature_accepts_lto_trimmed_results() {
        let function_type =
            FunctionType::new(CallConv::Wasm, [Type::Felt, Type::Felt], [Type::Felt]);
        let stub_signature = signature([Type::Felt, Type::Felt], []);

        assert!(stub_signature_matches_function_type(&stub_signature, &function_type));
    }

    #[test]
    fn stub_signature_accepts_heap_base_wasm_pointer_result() {
        let intrinsic = Intrinsic::Mem(Symbol::from(HEAP_BASE));
        let function_type = intrinsic.conversion_result().unwrap().function_type().clone();
        let stub_signature = signature([], [Type::I32]);

        assert!(stub_signature_matches_function_type(&stub_signature, &function_type));
    }

    #[test]
    fn stub_signature_rejects_param_mismatches() {
        let function_type =
            FunctionType::new(CallConv::Wasm, [Type::Felt, Type::Felt], [Type::Felt]);
        let stub_signature = signature([Type::Felt], []);

        assert!(!stub_signature_matches_function_type(&stub_signature, &function_type));
    }

    #[test]
    fn stub_signature_rejects_result_type_mismatches() {
        let function_type =
            FunctionType::new(CallConv::Wasm, [Type::Felt, Type::Felt], [Type::Felt]);
        let stub_signature = signature([Type::Felt, Type::Felt], [Type::I32]);

        assert!(!stub_signature_matches_function_type(&stub_signature, &function_type));
    }

    #[test]
    fn stub_signature_rejects_extra_results() {
        let function_type =
            FunctionType::new(CallConv::Wasm, [Type::Felt, Type::Felt], [Type::Felt]);
        let stub_signature = signature([Type::Felt, Type::Felt], [Type::Felt, Type::Felt]);

        assert!(!stub_signature_matches_function_type(&stub_signature, &function_type));
    }

    #[test]
    fn recognized_intrinsic_without_lowering_is_error() {
        let intrinsic = Intrinsic::Mem(Symbol::from("not_registered"));
        let import_path = intrinsic.into_symbol_path();

        assert!(
            require_intrinsic_conversion_result(
                "intrinsics::mem::not_registered",
                &import_path,
                intrinsic,
            )
            .is_err()
        );
    }

    #[test]
    fn recognized_stub_signature_mismatch_is_error() {
        let function_type =
            FunctionType::new(CallConv::Wasm, [Type::Felt, Type::Felt], [Type::Felt]);
        let stub_signature = signature([Type::Felt, Type::Felt], [Type::I32]);

        assert!(
            require_stub_signature_matches_function_type(
                "intrinsics::felt::add",
                &stub_signature,
                &function_type,
            )
            .is_err()
        );
    }

    #[test]
    fn recognized_miden_abi_path_without_signature_is_error() {
        let import_path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(symbols::Miden),
            SymbolNameComponent::Component(symbols::Protocol),
            SymbolNameComponent::Component(symbols::Tx),
            SymbolNameComponent::Leaf(Symbol::from("not_registered")),
        ]);

        assert!(
            require_miden_abi_function_type("miden::protocol::tx::not_registered", &import_path,)
                .is_err()
        );
    }
}

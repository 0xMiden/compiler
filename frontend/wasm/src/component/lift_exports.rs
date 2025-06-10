use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    dialects::builtin::{BuiltinOpBuilder, ComponentBuilder, ModuleBuilder},
    interner::Symbol,
    CallConv, FunctionType, Ident, Op, Signature, SmallVec, SourceSpan, SymbolPath, ValueRange,
    ValueRef,
};
use midenc_session::{diagnostics::Severity, DiagnosticsHandler};

use super::{
    canon_abi_utils::load,
    flat::{flatten_function_type, flatten_types, needs_transformation},
};
use crate::{
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

pub fn generate_export_lifting_function(
    component_builder: &mut ComponentBuilder,
    export_func_name: &str,
    export_func_ty: FunctionType,
    core_export_func_path: SymbolPath,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    let cross_ctx_export_sig_flat = flatten_function_type(&export_func_ty, CallConv::CanonLift)
        .map_err(|e| {
            let message = format!(
                "Component export lifting generation. Signature for exported function {} requires \
                 flattening. Error: {}",
                core_export_func_path, e
            );
            diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
        })?;

    let export_func_ident =
        Ident::new(Symbol::intern(export_func_name.to_string()), SourceSpan::default());

    let core_export_module_path = core_export_func_path.without_leaf();
    let core_module_ref = component_builder
        .resolve_module(&core_export_module_path)
        .expect("failed to find the core module");

    let core_module_builder = ModuleBuilder::new(core_module_ref);
    let core_export_func_ref = core_module_builder
        .get_function(core_export_func_path.name().as_str())
        .expect("failed to find the core module function");
    let core_export_func_sig = core_export_func_ref.borrow().signature().clone();

    if needs_transformation(&cross_ctx_export_sig_flat) {
        generate_lifting_with_transformation(
            component_builder,
            export_func_ident,
            &export_func_ty,
            cross_ctx_export_sig_flat,
            core_export_func_ref,
            core_export_func_sig,
            &core_export_func_path,
            diagnostics,
        )?;
    } else {
        generate_direct_lifting(
            component_builder,
            export_func_ident,
            core_export_func_ref,
            core_export_func_sig,
            cross_ctx_export_sig_flat,
        )?;
    }

    Ok(())
}

/// Generates a lifting function for component exports that require transformation.
///
/// This function handles the case where a core WebAssembly export needs to be "lifted" to match
/// Component Model conventions, specifically when the function returns complex types that exceed
/// the canonical ABI limits (e.g., structs with more than one field, or types larger than 64 bits).
///
/// In the transformation case, the core WASM function returns a pointer to the result data,
/// while the lifted component function returns the actual structured data as a tuple.
///
/// # Arguments
///
/// * `export_func_ident` - The identifier (name) for the exported function in the component interface.
///   This is the name that external callers will use.
///
/// * `export_func_ty` - The original function type from the component model perspective, containing
///   the high-level types (e.g., structs, records) before flattening.
///
/// * `cross_ctx_export_sig_flat` - The flattened component level export function signature after
///   applying canonical ABI transformations. This signature represents how the function appears in
///   cross-context calls.
///
/// * `core_export_func_ref` - Reference to the lowered core WebAssembly function that implements the actual
///   logic. This function follows core WASM conventions (returns pointer for complex types).
///
/// * `core_export_func_sig` - The signature of the lowered core WASM function, which may use pointer
///   returns for complex types according to canonical ABI rules.
///
/// * `core_export_func_path` - The symbol path to the core function, used for debugging and
///   error reporting purposes.
#[allow(clippy::too_many_arguments)]
fn generate_lifting_with_transformation(
    component_builder: &mut ComponentBuilder,
    export_func_ident: Ident,
    export_func_ty: &FunctionType,
    cross_ctx_export_sig_flat: Signature,
    core_export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    core_export_func_sig: Signature,
    core_export_func_path: &SymbolPath,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    assert_eq!(cross_ctx_export_sig_flat.results().len(), 1, "expected only one result");
    assert!(
        cross_ctx_export_sig_flat.results()[0].purpose == midenc_hir::ArgumentPurpose::StructReturn,
        "expected pointer in the result"
    );

    // Extract flattened result types from the exported component-level function type
    let flattened_results = flatten_types(&export_func_ty.results).map_err(|e| {
        let message = format!(
            "Failed to flatten result types for exported function {}: {}",
            core_export_func_path, e
        );
        diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
    })?;

    assert!(
        cross_ctx_export_sig_flat.params().len() <= 16,
        "only up to 16 felt flattened params supported (advice provider is not yet supported). \
         Try passing less data as a temporary workaround."
    );

    // Create the signature with the flattened result types
    let new_func_sig = Signature {
        params: cross_ctx_export_sig_flat.params,
        results: flattened_results.clone(),
        cc: cross_ctx_export_sig_flat.cc,
        visibility: cross_ctx_export_sig_flat.visibility,
    };
    let export_func_ref = component_builder.define_function(export_func_ident, new_func_sig)?;

    let (span, context) = {
        let export_func = export_func_ref.borrow();
        (export_func.name().span, export_func.as_operation().context_rc())
    };
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut fb = FunctionBuilderExt::new(export_func_ref, &mut op_builder);

    let entry_block = fb.current_block();
    fb.seal_block(entry_block);
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    // Export lifting: The core exported function returns a pointer to the result
    // We need to:
    // 1. Load the data from that pointer into the "flattened" representation (primitive types)
    // 2. Return it as individual values (tuple)

    let exec = fb.exec(core_export_func_ref, core_export_func_sig, args, span)?;

    let borrow = exec.borrow();
    let results = borrow.results().all();

    // The core function should return a single pointer (as i32)
    assert_eq!(results.len(), 1, "expected single result");
    let result_ptr = results.into_iter().next().unwrap().borrow().as_value_ref();

    // Load values from the core function's result pointer using recursive loading
    let mut return_values = SmallVec::<[ValueRef; 8]>::new();
    let mut offset = 0u32;

    // Load results using the recursive function from canon_abi_utils
    assert_eq!(
        export_func_ty.results.len(),
        1,
        "expected a single result in the component-level export function"
    );
    let result_type = &export_func_ty.results[0];

    load(&mut fb, result_ptr, result_type, &mut offset, &mut return_values, span)?;

    assert!(
        return_values.len() <= 16,
        "lift_exports: too many return values to pass on the stack, advice provider is not \
         supported. Try return less data as a temporary workaround."
    );

    // Return the loaded values
    let exit_block = fb.create_block();
    fb.br(exit_block, return_values.clone(), span)?;
    fb.append_block_params_for_function_returns(exit_block);
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    fb.ret(return_values, span)?;

    Ok(())
}

/// Generates a lifting function for component exports that don't require transformation.
///
/// This function handles the simple case where a core WebAssembly export can be directly
/// lifted to a component export without any signature transformation. This occurs when:
/// - The function returns a single primitive value (fits in 64 bits)
/// - The function returns nothing (void)
/// - The types are already compatible between core WASM and Component Model
///
/// # Arguments
///
/// * `export_func_ident` - The identifier (name) for the exported function. This name will be
///   used by external callers to invoke the function.
///
/// * `core_export_func_ref` - Reference to the underlying lowered core WebAssembly function that provides
///   the actual implementation. This function is called directly without transformation.
///
/// * `core_export_func_sig` - The signature of the lowered core function, which is compatible with the
///   component model signature (no transformation needed).
///
/// * `cross_ctx_export_sig_flat` - The flattened component level export function signature after
///   applying canonical ABI transformations. This signature represents how the function appears in
///   cross-context calls.
///
/// The generated lifting function is essentially a simple wrapper that:
/// 1. Receives arguments from the component model caller
/// 2. Directly calls the core WASM function with the same arguments
/// 3. Returns the result unchanged
///
fn generate_direct_lifting(
    component_builder: &mut ComponentBuilder,
    export_func_ident: Ident,
    core_export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    core_export_func_sig: Signature,
    cross_ctx_export_sig_flat: Signature,
) -> WasmResult<()> {
    let export_func_ref =
        component_builder.define_function(export_func_ident, cross_ctx_export_sig_flat.clone())?;

    let (span, context) = {
        let export_func = export_func_ref.borrow();
        (export_func.name().span, export_func.as_operation().context_rc())
    };
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut fb = FunctionBuilderExt::new(export_func_ref, &mut op_builder);

    let entry_block = fb.current_block();
    fb.seal_block(entry_block);
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    let exec = fb
        .exec(core_export_func_ref, core_export_func_sig, args, span)
        .expect("failed to build an exec op");

    let borrow = exec.borrow();
    let results = ValueRange::<2>::from(borrow.results().all());
    assert!(results.len() <= 1, "expected a single result or none");

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span).expect("failed br");
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning_onty_first = results.iter().take(1);
    fb.ret(returning_onty_first, span).expect("failed ret");

    Ok(())
}

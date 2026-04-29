use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_frontend_wasm_metadata::ProtocolExportKind;
use midenc_hir::{
    DICompileUnit, DISubprogram, FunctionType, Ident, Op, OpExt, SmallVec, SourceSpan, SymbolPath,
    ValueRange, ValueRef, Visibility,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ModuleBuilder,
        attributes::{Signature, UnitAttr},
    },
};
use midenc_session::{DiagnosticsHandler, diagnostics::Severity};

use super::{
    canon_abi_utils::load,
    flat::{CanonicalAbiMode, flatten_function_type, flatten_types, needs_transformation},
};
use crate::{
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

struct ComponentExportMetadata<'a> {
    ty: &'a FunctionType,
    param_names: &'a [String],
    protocol_export_kind: Option<ProtocolExportKind>,
}

/// Generates a lifted component export wrapper around a lowered core Wasm export.
pub fn generate_export_lifting_function(
    component_builder: &mut ComponentBuilder,
    export_func_name: &str,
    export_func_ty: FunctionType,
    export_param_names: &[String],
    core_export_func_path: SymbolPath,
    protocol_export_kind: Option<ProtocolExportKind>,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    let context = { component_builder.component.borrow().as_operation().context_rc() };
    let cross_ctx_export_sig_flat =
        flatten_function_type(&context, &export_func_ty, CanonicalAbiMode::Export).map_err(
            |e| {
                let message = format!(
                    "Component export lifting generation. Signature for exported function \
                     {core_export_func_path} requires flattening. Error: {e}"
                );
                diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
            },
        )?;

    if cross_ctx_export_sig_flat.params().iter().any(|param| param.ty.is_pointer()) {
        let message = format!(
            "component export lifting for '{core_export_func_path}' is not yet implemented for \
             passing the parameters using the advice provider in the cross-context `call`;"
        );
        return Err(diagnostics.diagnostic(Severity::Error).with_message(message).into_report());
    }

    let export_func_ident = Ident::new(export_func_name.to_string().into(), SourceSpan::default());
    let export_metadata = ComponentExportMetadata {
        ty: &export_func_ty,
        param_names: export_param_names,
        protocol_export_kind,
    };

    let core_export_module_path = core_export_func_path.without_leaf();
    let core_module_ref = component_builder
        .resolve_module(&core_export_module_path)
        .expect("failed to find the core module");

    let mut core_module_builder = ModuleBuilder::new(core_module_ref);
    let core_export_func_ref = core_module_builder
        .get_function(core_export_func_path.name().as_str())
        .expect("failed to find the core module export function");
    // Make the lowered core WASM export private so only the lifted wrapper is
    // publicly exported from the component. This prevents double-exports and
    // ensures all external callers go through the Canonical ABI–correct
    // wrapper generated here.
    core_module_builder
        .set_function_visibility(core_export_func_path.name().as_str(), Visibility::Private);
    let core_export_func_sig = core_export_func_ref.borrow().get_signature().clone();

    if needs_transformation(&cross_ctx_export_sig_flat) {
        generate_lifting_with_transformation(
            component_builder,
            export_func_ident,
            &export_metadata,
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
            &export_metadata,
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
    export_metadata: &ComponentExportMetadata<'_>,
    cross_ctx_export_sig_flat: Signature,
    core_export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    core_export_func_sig: Signature,
    core_export_func_path: &SymbolPath,
    diagnostics: &DiagnosticsHandler,
) -> WasmResult<()> {
    assert_eq!(
        cross_ctx_export_sig_flat.results().len(),
        1,
        "The flattened signature for {export_func_ident} component export function is expected to \
         have only one result",
    );
    assert!(
        cross_ctx_export_sig_flat.results()[0].is_sret_param(),
        "The flattened signature for {export_func_ident} component export function is expected to \
         have a pointer in the result",
    );

    // Extract flattened result types from the exported component-level function type
    let context = { core_export_func_ref.borrow().as_operation().context_rc() };
    let flattened_results = flatten_types(&context, &export_metadata.ty.results).map_err(|e| {
        let message = format!(
            "Failed to flatten result types for exported function {core_export_func_path}: {e}"
        );
        diagnostics.diagnostic(Severity::Error).with_message(message).into_report()
    })?;

    assert!(
        cross_ctx_export_sig_flat.params().len() <= 16,
        "Too many parameters in the flattened signature of {export_func_ident} component export \
         function. For cross-context calls only up to 16 felt flattened params supported (advice \
         provider is not yet supported). Try passing less data as a temporary workaround.",
    );

    // Create the signature with the flattened result types
    let new_func_sig = Signature {
        params: cross_ctx_export_sig_flat.params,
        results: flattened_results.clone(),
        cc: cross_ctx_export_sig_flat.cc,
    };
    let export_func_ref =
        component_builder.define_function(export_func_ident, Visibility::Public, new_func_sig)?;
    annotate_protocol_export(export_func_ref, export_metadata.protocol_export_kind);
    annotate_component_export_debug_signature(
        export_func_ref,
        export_func_ident.name.as_str(),
        export_metadata.ty,
        export_metadata.param_names,
    );

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
    let result_ptr = results[0].borrow().as_value_ref();

    // Load values from the core function's result pointer using recursive loading
    let mut return_values = SmallVec::<[ValueRef; 8]>::new();

    // Load results using the recursive function from canon_abi_utils
    assert_eq!(
        export_metadata.ty.results.len(),
        1,
        "expected a single result in the component-level export function"
    );
    let result_type = &export_metadata.ty.results[0];

    load(&mut fb, result_ptr, result_type, &mut return_values, span)?;

    assert!(
        return_values.len() <= 16,
        "Too many return values to pass on the stack for lifted {export_func_ident} component \
         export function. The advice provider is not supported. Try return less data as a \
         temporary workaround."
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
    export_metadata: &ComponentExportMetadata<'_>,
    core_export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    core_export_func_sig: Signature,
    cross_ctx_export_sig_flat: Signature,
) -> WasmResult<()> {
    let export_func_ref = component_builder.define_function(
        export_func_ident,
        Visibility::Public,
        cross_ctx_export_sig_flat.clone(),
    )?;
    annotate_protocol_export(export_func_ref, export_metadata.protocol_export_kind);
    annotate_component_export_debug_signature(
        export_func_ref,
        export_func_ident.name.as_str(),
        export_metadata.ty,
        export_metadata.param_names,
    );

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
    assert!(
        results.len() <= 1,
        "For direct lifting of the component export function {export_func_ident} expected a \
         single result or none"
    );

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span).expect("failed br");
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning_onty_first = results.iter().take(1);
    fb.ret(returning_onty_first, span).expect("failed ret");

    Ok(())
}

/// Marks lifted protocol exports with the attributes required by downstream consumers.
fn annotate_protocol_export(
    mut export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    protocol_export_kind: Option<ProtocolExportKind>,
) {
    let context = {
        let export_func = export_func_ref.borrow();
        export_func.as_operation().context_rc()
    };

    let mut export_func = export_func_ref.borrow_mut();
    match protocol_export_kind {
        Some(ProtocolExportKind::NoteScript) => {
            let note_attr = context.create_attribute::<UnitAttr, _>(());
            export_func.set_attribute("note_script", note_attr);
        }
        Some(ProtocolExportKind::AuthScript) => {
            let auth_attr = context.create_attribute::<UnitAttr, _>(());
            export_func.set_attribute("auth_script", auth_attr);
        }
        None => {}
    }
}

fn annotate_component_export_debug_signature(
    mut export_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    export_func_name: &str,
    export_func_ty: &FunctionType,
    export_param_names: &[String],
) {
    let context = {
        let export_func = export_func_ref.borrow();
        export_func.as_operation().context_rc()
    };

    let file = midenc_hir::interner::Symbol::intern("<component>");
    let mut compile_unit = DICompileUnit::new(midenc_hir::interner::Symbol::intern("wit"), file);
    compile_unit.producer = Some(midenc_hir::interner::Symbol::intern("midenc-frontend-wasm"));

    let param_names = export_param_names
        .iter()
        .map(|name| midenc_hir::interner::Symbol::intern(name.as_str()));
    let subprogram =
        DISubprogram::new(midenc_hir::interner::Symbol::intern(export_func_name), file, 1, Some(1))
            .with_function_type(FunctionType {
                abi: export_func_ty.abi,
                params: export_func_ty.params.clone(),
                results: export_func_ty.results.clone(),
            })
            .with_param_names(param_names);

    let cu_attr = context
        .create_attribute::<midenc_hir::DICompileUnitAttr, _>(compile_unit)
        .as_attribute_ref();
    let sp_attr = context
        .create_attribute::<midenc_hir::DISubprogramAttr, _>(subprogram)
        .as_attribute_ref();

    let mut export_func = export_func_ref.borrow_mut();
    let op = export_func.as_operation_mut();
    op.set_attribute("di.compile_unit", cu_attr);
    op.set_attribute("di.subprogram", sp_attr);
}

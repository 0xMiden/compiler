//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ComponentId, ModuleBuilder, WorldBuilder,
    },
    ArgumentPurpose, AsValueRange, CallConv, FunctionType, Op, Signature, SourceSpan, SymbolPath,
    ValueRef,
};

use super::{
    canon_abi_utils::store,
    flat::{flatten_function_type, flatten_types, needs_transformation},
};
use crate::{
    callable::CallableFunction,
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import function.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import_func_path: SymbolPath,
    import_func_ty: &FunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
) -> WasmResult<CallableFunction> {
    let import_lowered_sig = flatten_function_type(import_func_ty, CallConv::CanonLower)
        .wrap_err_with(|| {
            format!(
                "failed to generate component import lowering: signature of '{import_func_path}' \
                 requires flattening"
            )
        })?;

    let core_func_ref = module_builder
        .define_function(core_func_path.name().into(), core_func_sig.clone())
        .expect("failed to define the core function");

    let (span, context) = {
        let core_func = core_func_ref.borrow();
        (core_func.name().span, core_func.as_operation().context_rc())
    };
    let func_ctx = Rc::new(RefCell::new(FunctionBuilderContext::new(context.clone())));
    let mut op_builder =
        midenc_hir::OpBuilder::new(context).with_listener(SSABuilderListener::new(func_ctx));
    let mut fb = FunctionBuilderExt::new(core_func_ref, &mut op_builder);

    let entry_block = fb.current_block();
    fb.seal_block(entry_block);
    let args: Vec<ValueRef> = entry_block
        .borrow()
        .arguments()
        .iter()
        .copied()
        .map(|ba| ba as ValueRef)
        .collect();

    if needs_transformation(&import_lowered_sig) {
        generate_lowering_with_transformation(
            world_builder,
            &import_func_path,
            import_func_ty,
            core_func_path,
            core_func_sig,
            import_lowered_sig,
            core_func_ref,
            &mut fb,
            &args,
            span,
        )
    } else {
        generate_direct_lowering(
            world_builder,
            &import_func_path,
            import_func_ty,
            core_func_path,
            core_func_sig,
            core_func_ref,
            &mut fb,
            &args,
            span,
        )
    }
}

/// Generates a lowering function for component imports that require transformation.
///
/// This function handles the case where a Component Model import needs to be "lowered" to match
/// core WebAssembly conventions. This is necessary when importing functions that return complex
/// types (structs, records, tuples) which must be transformed to use pointer-based returns in
/// core WASM due to canonical ABI limitations.
///
/// The transformation converts from Component Model style (returning structured data) to core
/// WASM style (storing results via an output pointer parameter).
///
/// # Arguments
///
/// * `import_func_path` - The full symbol path to the imported function, including namespace,
///   component name, and function name (e.g., "miden:component/interface@1.0.0#function").
///
/// * `import_func_ty` - The original Component Model function type with high-level types
///   (structs, records) before any flattening or transformation.
///
/// * `core_func_path` - The symbol path for the core WASM function being generated. This is
///   the lowered function that will be called from core WASM code.
///
/// * `core_func_sig` - The signature of the generated lowered core function, which includes a pointer
///   parameter for returning complex results according to canonical ABI rules.
///
/// * `import_func_sig_flat` - The flattened signature after applying canonical lowering. Contains
///   the pointer parameter for struct returns when needed.
///
/// * `core_func_ref` - Reference to the core function being built. This is the function that
///   will contain the lowering logic.
///
/// * `args` - The arguments passed to the core function, including the output pointer as the
///   last argument for storing results.
///
#[allow(clippy::too_many_arguments)]
fn generate_lowering_with_transformation(
    world_builder: &mut WorldBuilder,
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    import_func_sig_flat: Signature,
    core_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<CallableFunction> {
    assert!(
        import_func_sig_flat.params().last().unwrap().purpose == ArgumentPurpose::StructReturn,
        "The flattened component import function {import_func_path} signature should have the \
         last parameter a pointer"
    );

    assert!(
        core_func_sig.results().is_empty(),
        "The lowered core function {core_func_path} should not have results when using \
         out-pointer pattern"
    );

    let id = ComponentId::try_from(import_func_path)
        .wrap_err("path does not start with a valid component id")?;
    let component_ref = if let Some(component_ref) = world_builder.find_component(&id) {
        component_ref
    } else {
        world_builder
            .define_component(id.namespace.into(), id.name.into(), id.version)
            .expect("failed to define the component")
    };

    let mut component_builder = ComponentBuilder::new(component_ref);

    // The import function's results are passed via a pointer parameter.
    // This happens when the result type would flatten to more than 1 value

    // The import function should have the lifted signature (returns tuple)
    // not the lowered signature with pointer parameter
    let import_func_sig = flatten_function_type(import_func_ty, CallConv::CanonLower)
        .wrap_err_with(|| {
            format!("failed to flatten import function signature for '{import_func_path}'")
        })?;

    // Extract the actual result types from the import function type
    let flattened_results = flatten_types(&import_func_ty.results).wrap_err_with(|| {
        format!("failed to flatten result types for import function '{import_func_path}'")
    })?;

    // Remove the pointer parameter that was added for the flattened signature
    let params_without_ptr = import_func_sig.params[..import_func_sig.params.len() - 1].to_vec();
    let new_import_func_sig = Signature {
        params: params_without_ptr,
        results: flattened_results.clone(),
        cc: import_func_sig.cc,
        visibility: import_func_sig.visibility,
    };
    let import_func_ref = component_builder
        .define_function(import_func_path.name().into(), new_import_func_sig.clone())
        .expect("failed to define the import function");

    // Import lowering: The lowered function takes a pointer as the last parameter
    // where results should be stored. The import function returns a pointer to the result.
    // We need to:
    // 1. Call the import function (it returns a tuple to the flattened result)
    // 2. Store the data from the tuple to the output pointer which expect to hold
    //    flattened result

    // Get the pointer argument (last argument) where we need to store results
    let output_ptr = args.last().expect("expected pointer argument");
    let args_without_ptr: Vec<_> = args[..args.len() - 1].to_vec();

    // Call the import function - it will return a tuple to the flattened result
    let call = fb.call(import_func_ref, new_import_func_sig, args_without_ptr, span)?;

    let borrow = call.borrow();
    let results = borrow.as_ref().results().as_value_range().into_owned();

    // Store values recursively based on the component-level type
    // This follows the canonical ABI store algorithm from:
    // https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#storing
    assert_eq!(import_func_ty.results.len(), 1, "expected a single result type");
    let result_type = &import_func_ty.results[0];
    let mut results_iter = results.into_iter();

    store(fb, *output_ptr, result_type, &mut results_iter, span)?;

    let exit_block = fb.create_block();
    fb.br(exit_block, [], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    fb.ret([], span)?;

    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}

/// Generates a lowering function for component imports that don't require transformation.
///
/// This function handles the simple case where a Component Model import can be directly
/// called from core WebAssembly without signature transformation. This occurs when:
/// - The function returns a single primitive value (fits in 64 bits)
/// - The function returns nothing (void)
/// - All parameters are simple types that don't need flattening
///
/// No pointer-based parameter passing or result storing is needed in this case.
///
/// # Arguments
///
/// * `import_func_path` - The full symbol path to the imported function in Component Model
///   format (e.g., "miden:component/interface@1.0.0#function").
///
/// * `import_func_ty` - The Component Model function type. In this case, it should be simple
///   enough to not require transformation.
///
/// * `core_func_path` - The symbol path for the generated core WASM function that performs
///   the lowering.
///
/// * `core_func_sig` - The lowered signature of the core function, which should be compatible with
///   the component import (no transformation needed).
///
/// * `core_func_ref` - Reference to the core function being built.
///
/// * `args` - The arguments to pass directly to the component import function.
///
/// # Implementation Details
///
/// The generated lowering function is a simple pass-through that:
/// 1. Receives arguments from core WASM caller
/// 2. Directly calls the component import with the same arguments
/// 3. Returns the result unchanged (at most one simple value)
///
#[allow(clippy::too_many_arguments)]
fn generate_direct_lowering(
    world_builder: &mut WorldBuilder,
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    core_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<CallableFunction> {
    let id = ComponentId::try_from(import_func_path)
        .wrap_err("path does not start with a valid component id")?;

    let component_ref = if let Some(component_ref) = world_builder.find_component(&id) {
        component_ref
    } else {
        world_builder
            .define_component(id.namespace.into(), id.name.into(), id.version)
            .expect("failed to define the component")
    };

    let mut component_builder = ComponentBuilder::new(component_ref);

    let import_func_sig = flatten_function_type(import_func_ty, CallConv::CanonLift)
        .wrap_err_with(|| {
            format!("failed to flatten import function signature for '{import_func_path}'")
        })?;
    let import_func_ref = component_builder
        .define_function(import_func_path.name().into(), import_func_sig.clone())
        .expect("failed to define the import function");

    let call = fb
        .call(import_func_ref, core_func_sig.clone(), args.to_vec(), span)
        .expect("failed to build an exec op");

    let borrow = call.borrow();
    let results_storage = borrow.as_ref().results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    assert!(
        results.len() <= 1,
        "For direct lowering the component import function {import_func_path} expected a single \
         result or none"
    );

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let returning = results.first().cloned();
    fb.ret(returning, span).expect("failed ret");

    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}

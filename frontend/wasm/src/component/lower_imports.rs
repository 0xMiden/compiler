//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{
    AsValueRange, Builder, CallConv, FunctionType, Op, OpExt, SourceSpan, SymbolNameComponent,
    SymbolPath, Type, ValueRef, Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ComponentId, FunctionRef, ModuleBuilder, WorldBuilder,
        attributes::{Signature, U32Attr},
    },
    interner::Symbol,
};

use super::{
    canon_abi_utils::store,
    flat::{CanonicalAbiMode, flatten_function_type, flatten_types, needs_transformation},
};
use crate::{
    callable::CallableFunction,
    error::WasmResult,
    miden_abi::tx_kernel::tx,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

const FPI_IMPORT_PREFIX: &str = "fpi-";
const FPI_ABI_PREFIX_ARGS: usize = 6;
const FPI_EXEC_INPUTS: usize = 16;
const FPI_EXEC_RESULTS: usize = 16;
const CANONICAL_ABI_MAX_FLAT_PARAMS: usize = 16;
const CANONICAL_ABI_MAX_FLAT_RESULTS: usize = 1;
const FPI_FLATTENED_ARG_COUNT_ATTR: &str = "fpi.flattened_arg_count";

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import function.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import_func_path: SymbolPath,
    import_func_ty: &FunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
) -> WasmResult<CallableFunction> {
    let context = module_builder.builder().context_rc();
    let import_lowered_sig =
        flatten_function_type(&context, import_func_ty, CanonicalAbiMode::Import).wrap_err_with(
            || {
                format!(
                    "failed to generate component import lowering: signature of \
                     '{import_func_path}' requires flattening"
                )
            },
        )?;

    let core_func_ref = module_builder
        .define_function(core_func_path.name().into(), Visibility::Internal, core_func_sig.clone())
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

    if is_fpi_import(&import_func_path) {
        return generate_fpi_lowering(
            world_builder,
            import_func_ty,
            &import_lowered_sig,
            core_func_path,
            core_func_sig,
            core_func_ref,
            &mut fb,
            &args,
            span,
        );
    }

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

/// Generates a lowering function for FPI imports backed by `execute_foreign_procedure`.
#[allow(clippy::too_many_arguments)]
fn generate_fpi_lowering(
    world_builder: &mut WorldBuilder,
    import_func_ty: &FunctionType,
    import_lowered_sig: &Signature,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    core_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<CallableFunction> {
    let context = world_builder.context_rc();
    let fpi_abi = lower_fpi_canonical_args(&context, import_func_ty, import_lowered_sig, args)
        .wrap_err_with(|| format!("failed to lower FPI import arguments for `{core_func_path}`"))?;
    validate_fpi_core_signature(
        &context,
        import_func_ty,
        &core_func_path,
        &core_func_sig,
        &fpi_abi,
    )?;

    let exec = match &fpi_abi.args {
        FpiExecArgs::Direct(fpi_args) => {
            let exec_func_ref = declare_execute_foreign_procedure(world_builder)?;

            let mut exec_args = Vec::with_capacity(2 + 4 + FPI_EXEC_INPUTS);
            let account_id_prefix = fpi_args[0];
            let account_id_suffix = fpi_args[1];
            let foreign_proc_root = &fpi_args[2..6];
            let procedure_inputs = &fpi_args[FPI_ABI_PREFIX_ARGS..];

            exec_args.push(account_id_suffix);
            exec_args.push(account_id_prefix);
            exec_args.extend(foreign_proc_root.iter().copied());
            exec_args.extend(procedure_inputs.iter().copied());

            let exec_sig = Signature::with_convention(
                &context,
                CallConv::Wasm,
                vec![Type::Felt; exec_args.len()],
                vec![Type::Felt; FPI_EXEC_RESULTS],
            );
            fb.exec(exec_func_ref, exec_sig, exec_args, span)?
        }
        FpiExecArgs::Indirect {
            arg_ptr,
            flattened_arg_count,
        } => {
            let exec_func_ref = declare_execute_foreign_procedure_indirect(world_builder)?;
            let exec_sig = Signature::with_convention(
                &context,
                CallConv::Wasm,
                vec![arg_ptr.borrow().ty().clone()],
                vec![Type::Felt; FPI_EXEC_RESULTS],
            );
            let mut exec = fb.exec(exec_func_ref, exec_sig, [*arg_ptr], span)?;
            let flattened_arg_count = u32::try_from(*flattened_arg_count)
                .expect("FPI flattened arg count must fit in u32");
            let flattened_arg_count_attr =
                context.create_attribute::<U32Attr, _>(flattened_arg_count);
            exec.borrow_mut()
                .set_attribute(FPI_FLATTENED_ARG_COUNT_ATTR, flattened_arg_count_attr);
            exec
        }
    };
    let mut results: Vec<ValueRef> = {
        let borrow = exec.borrow();
        borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
    };

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    if let Some(output_ptr) = fpi_abi.output_ptr {
        assert_eq!(import_func_ty.results.len(), 1, "expected a single FPI result type");
        let flattened_results =
            flatten_types(&context, &import_func_ty.results).wrap_err_with(|| {
                format!("failed to flatten FPI import results for `{core_func_path}`")
            })?;

        results.truncate(flattened_results.len());
        let mut results_iter = results.into_iter();
        store(fb, output_ptr, &import_func_ty.results[0], &mut results_iter, span)?;
        fb.ret([], span)?;
    } else {
        results.truncate(core_func_sig.results().len());
        fb.ret(results, span)?;
    }

    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}

/// Lowered arguments for an FPI import after accounting for canonical ABI pointers.
struct LoweredFpiAbi {
    /// Arguments to pass to the protocol executor.
    args: FpiExecArgs,
    /// Optional canonical ABI pointer where multi-felt results must be stored.
    output_ptr: Option<ValueRef>,
}

/// Protocol executor argument source for an FPI import.
enum FpiExecArgs {
    /// Fully flattened FPI arguments: account id prefix, account id suffix, root word, then inputs.
    Direct(Vec<ValueRef>),
    /// Canonical ABI argument tuple pointer plus the number of flattened values it contains.
    Indirect {
        /// Pointer to the canonical ABI argument tuple.
        arg_ptr: ValueRef,
        /// Number of flattened FPI argument felts in the tuple.
        flattened_arg_count: usize,
    },
}

/// Lowers canonical ABI FPI import arguments to the felt-only protocol ABI.
fn lower_fpi_canonical_args(
    context: &Rc<midenc_hir::Context>,
    import_func_ty: &FunctionType,
    import_lowered_sig: &Signature,
    args: &[ValueRef],
) -> WasmResult<LoweredFpiAbi> {
    let flattened_params = flatten_types(context, &import_func_ty.params)?;
    let flattened_results = flatten_types(context, &import_func_ty.results)?;
    let has_arg_ptr = flattened_params.len() > CANONICAL_ABI_MAX_FLAT_PARAMS;
    let has_output_ptr = flattened_results.len() > CANONICAL_ABI_MAX_FLAT_RESULTS;

    let expected_params = if has_arg_ptr {
        1 + usize::from(has_output_ptr)
    } else {
        flattened_params.len() + usize::from(has_output_ptr)
    };
    if args.len() != expected_params || import_lowered_sig.params().len() != expected_params {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import lowered to an unexpected core ABI shape: expected {expected_params} \
             params, got {}",
            args.len()
        )));
    }

    if has_arg_ptr && !import_lowered_sig.params()[0].ty.is_pointer() {
        return Err(midenc_session::diagnostics::Report::msg(
            "FPI import with more than 16 flattened params must lower to an argument pointer",
        ));
    }
    if has_output_ptr {
        let output_param = &import_lowered_sig.params()[expected_params - 1];
        if !output_param.ty.is_pointer() {
            return Err(midenc_session::diagnostics::Report::msg(
                "FPI import with more than one flattened result must lower to an output pointer",
            ));
        }
    }

    let output_ptr = has_output_ptr.then(|| *args.last().expect("expected FPI output pointer"));
    let args = if has_arg_ptr {
        let arg_ptr = *args.first().expect("expected FPI argument tuple pointer");
        FpiExecArgs::Indirect {
            arg_ptr,
            flattened_arg_count: flattened_params.len(),
        }
    } else {
        FpiExecArgs::Direct(args[..flattened_params.len()].to_vec())
    };

    Ok(LoweredFpiAbi { args, output_ptr })
}

/// Returns true for WIT imports generated for foreign procedure invocation.
fn is_fpi_import(import_func_path: &SymbolPath) -> bool {
    import_func_path.name().as_str().starts_with(FPI_IMPORT_PREFIX)
}

/// Validates the FPI import ABI that the Rust wrapper generates.
fn validate_fpi_core_signature(
    context: &Rc<midenc_hir::Context>,
    import_func_ty: &FunctionType,
    core_func_path: &SymbolPath,
    core_func_sig: &Signature,
    fpi_abi: &LoweredFpiAbi,
) -> WasmResult<()> {
    let flattened_arg_count = match &fpi_abi.args {
        FpiExecArgs::Direct(args) => args.len(),
        FpiExecArgs::Indirect {
            flattened_arg_count,
            ..
        } => *flattened_arg_count,
    };
    let procedure_input_count = flattened_arg_count.saturating_sub(FPI_ABI_PREFIX_ARGS);
    if flattened_arg_count < FPI_ABI_PREFIX_ARGS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` must pass account id and procedure root"
        )));
    }
    if procedure_input_count > FPI_EXEC_INPUTS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` passes {procedure_input_count} procedure input felts, \
             but `execute_foreign_procedure` supports at most {FPI_EXEC_INPUTS}"
        )));
    }

    let flattened_results = flatten_types(context, &import_func_ty.results)?;
    if flattened_results.len() > FPI_EXEC_RESULTS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` returns {} result felts, but \
             `execute_foreign_procedure` supports at most {FPI_EXEC_RESULTS}",
            flattened_results.len()
        )));
    }

    let has_output_ptr = fpi_abi.output_ptr.is_some();
    if !has_output_ptr && core_func_sig.results().len() > FPI_EXEC_RESULTS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` returns more than {FPI_EXEC_RESULTS} felts"
        )));
    }

    if has_output_ptr && !core_func_sig.results().is_empty() {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` with an output pointer must not also return values"
        )));
    }

    let all_fpi_params_are_felts = match &fpi_abi.args {
        FpiExecArgs::Direct(args) => args.iter().all(|arg| arg.borrow().ty() == &Type::Felt),
        FpiExecArgs::Indirect { .. } => flatten_types(context, &import_func_ty.params)?
            .iter()
            .all(|param| param.ty == Type::Felt),
    };
    let all_results_are_felts =
        core_func_sig.results().iter().all(|result| result.ty == Type::Felt);
    if !all_fpi_params_are_felts || !all_results_are_felts {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` must lower to felt-only parameters and results"
        )));
    }

    Ok(())
}

/// Declares the tx kernel FPI executor and returns its HIR function reference and signature.
fn declare_execute_foreign_procedure(world_builder: &mut WorldBuilder) -> WasmResult<FunctionRef> {
    let exec_path = execute_foreign_procedure_path();
    let context = world_builder.context_rc();
    let signature = Signature::with_convention(
        &context,
        CallConv::Wasm,
        vec![Type::Felt; 2 + 4 + FPI_EXEC_INPUTS],
        vec![Type::Felt; FPI_EXEC_RESULTS],
    );
    let import_module_ref = world_builder
        .declare_module_tree(&exec_path.without_leaf())
        .wrap_err("failed to create tx module for FPI imports")?;
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let function_name = exec_path.name().as_str();
    let function_ref = if let Some(function_ref) = import_module_builder.get_function(function_name)
    {
        function_ref
    } else {
        import_module_builder
            .define_function(exec_path.name().into(), Visibility::Public, signature.clone())
            .wrap_err("failed to create FPI executor function ref")?
    };

    Ok(function_ref)
}

/// Declares the compiler-internal indirect FPI executor used for canonical ABI argument pointers.
fn declare_execute_foreign_procedure_indirect(
    world_builder: &mut WorldBuilder,
) -> WasmResult<FunctionRef> {
    let exec_path = execute_foreign_procedure_indirect_path();
    let context = world_builder.context_rc();
    let signature = Signature::with_convention(
        &context,
        CallConv::Wasm,
        vec![Type::I32],
        vec![Type::Felt; FPI_EXEC_RESULTS],
    );
    let import_module_ref = world_builder
        .declare_module_tree(&exec_path.without_leaf())
        .wrap_err("failed to create tx module for indirect FPI imports")?;
    let mut import_module_builder = ModuleBuilder::new(import_module_ref);
    let function_name = exec_path.name().as_str();
    let function_ref = if let Some(function_ref) = import_module_builder.get_function(function_name)
    {
        function_ref
    } else {
        import_module_builder
            .define_function(exec_path.name().into(), Visibility::Public, signature)
            .wrap_err("failed to create indirect FPI executor function ref")?
    };

    Ok(function_ref)
}

/// Fully-qualified MASM path for `miden::protocol::tx::execute_foreign_procedure`.
fn execute_foreign_procedure_path() -> SymbolPath {
    SymbolPath::from_iter(
        tx::MODULE_PREFIX
            .iter()
            .copied()
            .chain([SymbolNameComponent::Leaf(Symbol::intern(tx::EXECUTE_FOREIGN_PROCEDURE))]),
    )
}

/// Compiler-internal path for indirect FPI calls lowered specially by the MASM backend.
fn execute_foreign_procedure_indirect_path() -> SymbolPath {
    SymbolPath::from_iter(
        tx::MODULE_PREFIX
            .iter()
            .copied()
            .chain([SymbolNameComponent::Leaf(Symbol::intern(
                "execute_foreign_procedure_indirect",
            ))]),
    )
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
        import_func_sig_flat.params().last().unwrap().is_sret_param(),
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
    let context = world_builder.context_rc();
    let import_func_sig = flatten_function_type(&context, import_func_ty, CanonicalAbiMode::Import)
        .wrap_err_with(|| {
            format!("failed to flatten import function signature for '{import_func_path}'")
        })?;

    // Extract the actual result types from the import function type
    let flattened_results =
        flatten_types(&context, &import_func_ty.results).wrap_err_with(|| {
            format!("failed to flatten result types for import function '{import_func_path}'")
        })?;

    // Remove the pointer parameter that was added for the flattened signature
    let params_without_ptr = import_func_sig.params[..import_func_sig.params.len() - 1].to_vec();
    let new_import_func_sig = Signature {
        params: params_without_ptr,
        results: flattened_results.clone(),
        cc: import_func_sig.cc,
    };
    let import_func_ref = component_builder
        .define_function(
            import_func_path.name().into(),
            Visibility::Internal,
            new_import_func_sig.clone(),
        )
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
    let results = borrow.results().as_value_range().into_owned();

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

    let context = world_builder.context_rc();
    let import_func_sig = flatten_function_type(&context, import_func_ty, CanonicalAbiMode::Import)
        .wrap_err_with(|| {
            format!("failed to flatten import function signature for '{import_func_path}'")
        })?;
    let import_func_ref = component_builder
        .define_function(
            import_func_path.name().into(),
            Visibility::Internal,
            import_func_sig.clone(),
        )
        .expect("failed to define the import function");

    let call = fb
        .call(import_func_ref, core_func_sig.clone(), args.to_vec(), span)
        .expect("failed to build an exec op");

    let borrow = call.borrow();
    let results_storage = borrow.results();
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

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::Context;

    use super::*;

    #[test]
    fn validate_fpi_core_signature_rejects_too_many_procedure_inputs() {
        let context = Rc::new(Context::default());
        let arg_block = context.create_block_with_params([Type::I32]);
        let arg_ptr = arg_block.borrow().arguments()[0] as ValueRef;
        let flattened_arg_count = FPI_ABI_PREFIX_ARGS + FPI_EXEC_INPUTS + 1;
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![Type::Felt; flattened_arg_count],
            vec![Type::Felt],
        );
        let core_func_path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(Symbol::intern("miden")),
            SymbolNameComponent::Component(Symbol::intern("too-many-args-account")),
            SymbolNameComponent::Leaf(Symbol::intern("fpi-get-count-sum-by-keys")),
        ]);
        let core_func_sig = Signature::with_convention(
            &context,
            CallConv::Wasm,
            vec![Type::I32],
            vec![Type::Felt; FPI_EXEC_RESULTS],
        );
        let fpi_abi = LoweredFpiAbi {
            args: FpiExecArgs::Indirect {
                arg_ptr,
                flattened_arg_count,
            },
            output_ptr: None,
        };

        let err = validate_fpi_core_signature(
            &context,
            &import_func_ty,
            &core_func_path,
            &core_func_sig,
            &fpi_abi,
        )
        .expect_err("expected FPI validation to reject more than sixteen procedure input felts");
        let message = err.to_string();

        assert!(
            message.contains("passes 17 procedure input felts")
                && message.contains("`execute_foreign_procedure` supports at most 16"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn validate_fpi_core_signature_rejects_too_many_results() {
        let context = Rc::new(Context::default());
        let block = context.create_block_with_params(
            (0..FPI_ABI_PREFIX_ARGS).map(|_| Type::Felt).chain([Type::I32]),
        );
        let args = block
            .borrow()
            .arguments()
            .iter()
            .take(FPI_ABI_PREFIX_ARGS)
            .map(|arg| *arg as ValueRef)
            .collect::<Vec<_>>();
        let output_ptr = block.borrow().arguments()[FPI_ABI_PREFIX_ARGS] as ValueRef;
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![Type::Felt; FPI_ABI_PREFIX_ARGS],
            vec![Type::Felt; FPI_EXEC_RESULTS + 1],
        );
        let core_func_path = SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(Symbol::intern("miden")),
            SymbolNameComponent::Component(Symbol::intern("too-many-results-account")),
            SymbolNameComponent::Leaf(Symbol::intern("fpi-get-count-words")),
        ]);
        let core_func_sig = Signature::with_convention(
            &context,
            CallConv::Wasm,
            vec![Type::Felt; FPI_ABI_PREFIX_ARGS + 1],
            vec![],
        );
        let fpi_abi = LoweredFpiAbi {
            args: FpiExecArgs::Direct(args),
            output_ptr: Some(output_ptr),
        };

        let err = validate_fpi_core_signature(
            &context,
            &import_func_ty,
            &core_func_path,
            &core_func_sig,
            &fpi_abi,
        )
        .expect_err("expected FPI validation to reject more than sixteen result felts");
        let message = err.to_string();

        assert!(
            message.contains("returns 17 result felts")
                && message.contains("`execute_foreign_procedure` supports at most 16"),
            "unexpected error message: {message}"
        );
    }
}

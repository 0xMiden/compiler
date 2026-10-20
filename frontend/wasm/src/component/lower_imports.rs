//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::{Dyncall, ExecFpi, HirOpBuilder};
use midenc_frontend_wasm_metadata::{DYNCALL_IMPORT_PREFIX, FPI_IMPORT_PREFIX};
use midenc_hir::{
    Builder, Context, FunctionType, Ident, Op, OpExt, SmallVec, SourceSpan, SymbolName, SymbolPath,
    SymbolTable, Type, ValueRef, Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, Function, FunctionRef, ModuleBuilder, WorldBuilder,
        attributes::{AbiParam, Signature, StringAttr},
    },
};
use midenc_session::diagnostics::Report;

use super::{
    ComponentFunctionType, MAX_DIRECT_STACK_FELTS, MAX_FLAT_RESULTS,
    canon_abi_utils::{load, offset_addr, store, validate_flat_variants},
    canonical_abi_info, canonical_flat_types, contains_unsupported_canonical_abi_type,
    flat::{
        CanonicalAbiIndirection, CanonicalAbiMode, check_core_wasm_signature_equivalence,
        classify_function_type, expected_core_signature, flat_params_need_tuple,
        flatten_function_type, flatten_types,
    },
};
use crate::{
    callable::CallableFunction,
    error::WasmResult,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

/// The attribute recording, on a declared import function, the component-model path of the
/// import that declared it, so a later clashing import can name it. Its value is a `StringAttr`.
const COMPONENT_IMPORT_PATH_ATTR: &str = "wasm_component_import_path";

const FPI_ABI_PREFIX_ARGS: usize = ExecFpi::PREFIX_FELTS;

/// Field elements the callee's procedure root occupies, both as the leading flat parameters of a
/// dyncall import's lowering function and as the root operand of the `hir.dyncall` it emits.
const DYNCALL_ROOT_FELTS: usize = Dyncall::ROOT_FELTS;

const FPI_EXEC_INPUTS: usize = ExecFpi::MAX_INPUT_FELTS;

const FPI_EXEC_RESULTS: usize = ExecFpi::EXECUTOR_RESULT_FELTS;

/// How a lowering function reaches the procedure behind a (non-FPI) component import.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ImportCallKind {
    /// A `hir.call` to the import declared on its component, resolved by path at assembly time.
    Call,
    /// A `hir.dyncall` to the MAST root passed as the import's leading `word` parameter; nothing
    /// is declared, as the target is runtime data.
    Dyncall,
}

/// The two paths of a component import function.
pub struct ComponentImportPath {
    /// The component-model path `::<interface id>::<function>`, which matches the core import
    /// to its `canon lower`; used to recognize FPI and dyncall imports and in diagnostics.
    pub cm_path: SymbolPath,
    /// The Miden path from the import's `external-id`; names the imported function, which is
    /// declared in the stub component at the path's parent unless the import is an FPI or
    /// dyncall import (those declare nothing).
    pub path: SymbolPath,
    /// The name (the Miden Assembly spelling of the namespace path, see
    /// `SymbolPath::to_symbol_name`) of the component being translated; the `path` of an import
    /// other than an FPI or dyncall import must lie outside of that namespace.
    pub namespace: SymbolName,
}

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import
/// function, defined in the core module as `stub_name`.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import: ComponentImportPath,
    import_func_ty: &ComponentFunctionType,
    core_func_path: SymbolPath,
    stub_name: SymbolName,
    core_func_sig: Signature,
) -> WasmResult<CallableFunction> {
    let import_func_path = &import.cm_path;
    let context = module_builder.builder().context_rc();
    // Special imports validate their typed protocol shape before canonical source flattening.
    let is_fpi = is_fpi_import(import_func_path, &import_func_ty.ir)?;
    let call_kind = if is_dyncall_import(import_func_path, &import_func_ty.ir)? {
        ImportCallKind::Dyncall
    } else {
        ImportCallKind::Call
    };
    if is_fpi {
        validate_fpi_typed_signature(import_func_path, &import_func_ty.ir)?;
    } else {
        reject_unsupported_import_canonical_abi_types(import_func_path, import_func_ty)?;
    }
    // Canonical source classification includes all parameters, including a dyncall root.
    let import_lowered_sig =
        flatten_function_type(&context, &import_func_ty.ir, CanonicalAbiMode::Import)
            .wrap_err_with(|| format!("failed to flatten component import '{import_func_path}'"))?;
    let transformation = if is_fpi {
        None
    } else {
        let transformation = classify_function_type(&context, &import_func_ty.ir)?;
        if call_kind == ImportCallKind::Call && transformation.has_param_tuple() {
            return reject_tuple_parameter_import_lowering(import_func_path);
        }
        check_core_wasm_signature_equivalence(
            &core_func_sig,
            &expected_core_signature(&import_lowered_sig),
        )
        .map_err(|message| {
            Report::msg(format!(
                "component import lowering for '{import_func_path}' has core Wasm signature \
                 mismatch: {message}"
            ))
        })?;
        let target_ty = match call_kind {
            ImportCallKind::Call => import_func_ty.ir.clone(),
            ImportCallKind::Dyncall => {
                split_dyncall_root(&context, import_func_path, &import_func_ty.ir)?.0
            }
        };
        validate_import_target_capacity(&context, import_func_path, &target_ty, call_kind)?;
        Some(transformation)
    };

    let core_func_ref = module_builder
        .define_function(stub_name.into(), Visibility::Internal, core_func_sig.clone())
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

    let Some(transformation) = transformation else {
        return generate_fpi_lowering(
            import_func_ty,
            &import_lowered_sig,
            core_func_path,
            core_func_sig,
            core_func_ref,
            &mut fb,
            &args,
            span,
        );
    };

    generate_lowering(
        world_builder,
        call_kind,
        &import,
        import_func_ty,
        core_func_path,
        core_func_sig,
        transformation,
        core_func_ref,
        &mut fb,
        &args,
        span,
    )
}

/// Generates a lowering function for FPI imports backed by `execute_foreign_procedure`.
#[allow(clippy::too_many_arguments)]
fn generate_fpi_lowering(
    import_func_ty: &ComponentFunctionType,
    import_lowered_sig: &Signature,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    core_func_ref: midenc_hir::dialects::builtin::FunctionRef,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<CallableFunction> {
    let context = core_func_ref.borrow().as_operation().context_rc();
    let shape = plan_fpi_call(
        &context,
        &import_func_ty.ir,
        import_lowered_sig,
        &core_func_path,
        &core_func_sig,
        args.len(),
    )?;
    let lowered = lower_fpi_canonical_args(&shape, import_func_ty, fb, args, span)
        .wrap_err_with(|| format!("failed to lower FPI import arguments for `{core_func_path}`"))?;

    // Wrappers carry prefix before suffix; the executor consumes suffix before prefix.
    let prefix = [
        lowered.fpi_args[1],
        lowered.fpi_args[0],
        lowered.fpi_args[2],
        lowered.fpi_args[3],
        lowered.fpi_args[4],
        lowered.fpi_args[5],
    ];
    let procedure_inputs = lowered.fpi_args[FPI_ABI_PREFIX_ARGS..].iter().copied();
    let exec = fb.exec_fpi(prefix, procedure_inputs, span)?;
    let results: Vec<ValueRef> = {
        let borrow = exec.borrow();
        borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
    };

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let results = lower_fpi_result_felts(fb, &shape.flattened_results, &results, span)?;
    // Result felts come from the foreign account, so variant discriminants are validated before
    // the values are stored or returned, mirroring the transformed component import path.
    validate_flat_variants(fb, &import_func_ty.ir.results, &results, span)?;
    if let Some(output_ptr) = lowered.output_ptr {
        if import_func_ty.ir.results.len() != 1 {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import with an output pointer expected one result type, got {}",
                import_func_ty.ir.results.len()
            )));
        }
        let mut results_iter = results.into_iter();
        store(fb, output_ptr, &import_func_ty.ir.results[0], &mut results_iter, span)?;
        fb.ret([], span)?;
    } else {
        fb.ret(results, span)?;
    }

    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}

/// Validated shape of an FPI import call, computed before any IR is emitted.
#[derive(Debug)]
struct FpiCallShape {
    /// Whether the canonical ABI passes the arguments through one tuple pointer.
    has_arg_ptr: bool,
    /// Whether the canonical ABI returns the results through an output pointer.
    has_output_ptr: bool,
    /// Canonical ABI flat parameters for the import function.
    flattened_params: Vec<AbiParam>,
    /// Canonical ABI flat results for the import function.
    flattened_results: Vec<AbiParam>,
    /// Number of protocol felts the flattened parameters expand to, including the 6-felt prefix.
    flattened_arg_felts: usize,
}

/// Emitted protocol arguments for a validated FPI call shape.
struct LoweredFpiAbi {
    /// Felt-only protocol arguments: the 6 wrapper-order prefix felts (account id prefix, account
    /// id suffix, procedure root), followed by the flattened procedure input felts.
    fpi_args: Vec<ValueRef>,
    /// Optional canonical ABI pointer where multi-felt results must be stored.
    output_ptr: Option<ValueRef>,
}

/// Validates that a typed FPI import signature contains only self-contained value types.
fn validate_fpi_typed_signature(
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
) -> WasmResult<()> {
    for (index, ty) in import_func_ty.params.iter().enumerate() {
        validate_fpi_value_type(import_func_path, &format!("parameter {index}"), ty)?;
    }
    for (index, ty) in import_func_ty.results.iter().enumerate() {
        validate_fpi_value_type(import_func_path, &format!("result {index}"), ty)?;
    }

    Ok(())
}

/// Validates that an FPI value type can be encoded as protocol felts.
fn validate_fpi_value_type(
    import_func_path: &SymbolPath,
    location: &str,
    ty: &Type,
) -> WasmResult<()> {
    match ty {
        Type::List(_) | Type::Ptr(_) => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import `{import_func_path}` {location} contains pointer-like type `{ty}`; \
                 typed FPI does not support list, string, or pointer values because they lower to \
                 caller linear-memory addresses"
            )));
        }
        Type::Struct(struct_ty) => {
            for field in struct_ty.get().fields() {
                let field_location = field.name.as_ref().map_or_else(
                    || format!("{location}.{}", field.index),
                    |name| format!("{location}.{name}"),
                );
                validate_fpi_value_type(import_func_path, &field_location, &field.ty)?;
            }
        }
        Type::Array(array_ty) => {
            validate_fpi_value_type(
                import_func_path,
                &format!("{location}[]"),
                array_ty.element_type(),
            )?;
        }
        Type::Enum(enum_ty) => {
            let enum_ty = enum_ty.get();
            validate_fpi_value_type(import_func_path, location, enum_ty.discriminant())?;
            for variant in enum_ty.variants() {
                if let Some(payload_ty) = variant.value.as_ref() {
                    validate_fpi_value_type(
                        import_func_path,
                        &format!("{location}::{}", variant.name),
                        payload_ty,
                    )?;
                }
            }
        }
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::I64
        | Type::U64
        | Type::Felt => {}
        Type::Unknown
        | Type::Variadic
        | Type::Never
        | Type::I128
        | Type::U128
        | Type::U256
        | Type::F64
        | Type::Function(_) => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import `{import_func_path}` {location} contains unsupported type `{ty}`; \
                 typed FPI only supports felt-width integers, felt, enums, structs, and arrays"
            )));
        }
    }

    Ok(())
}

/// Computes and validates the shape of an FPI import call before any IR is emitted.
fn plan_fpi_call(
    context: &Rc<midenc_hir::Context>,
    import_func_ty: &FunctionType,
    import_lowered_sig: &Signature,
    core_func_path: &SymbolPath,
    core_func_sig: &Signature,
    num_args: usize,
) -> WasmResult<FpiCallShape> {
    let flattened_params = flatten_types(context, &import_func_ty.params)?;
    let flattened_results = flatten_types(context, &import_func_ty.results)?;
    // Source classification counts canonical values; protocol capacity counts expanded felts.
    let has_arg_ptr = flat_params_need_tuple(&flattened_params);
    let has_output_ptr = flattened_results.len() > MAX_FLAT_RESULTS;

    let expected_params = if has_arg_ptr {
        1 + usize::from(has_output_ptr)
    } else {
        flattened_params.len() + usize::from(has_output_ptr)
    };
    if num_args != expected_params || import_lowered_sig.params().len() != expected_params {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import lowered to an unexpected core ABI shape: expected {expected_params} \
             params, got {num_args}"
        )));
    }

    if has_arg_ptr && !import_lowered_sig.params()[0].ty.is_pointer() {
        return Err(midenc_session::diagnostics::Report::msg(
            "FPI import exceeding the direct parameter budget must lower to an argument pointer",
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

    // Mirror the non-FPI lowering paths: the core import must carry the canonical lowered
    // parameter and result types, with canonical pointer parameters passed as core `i32`
    // values. Arity checks alone would let mismatched scalar widths reach felt conversion.
    let expected_core_sig = expected_core_signature(import_lowered_sig);
    check_core_wasm_signature_equivalence(core_func_sig, &expected_core_sig).map_err(
        |message| {
            midenc_session::diagnostics::Report::msg(format!(
                "FPI import `{core_func_path}` has core Wasm signature mismatch: {message}"
            ))
        },
    )?;

    let flattened_arg_felts = fpi_flat_value_count(&flattened_params)?;
    let procedure_input_count = flattened_arg_felts.saturating_sub(FPI_ABI_PREFIX_ARGS);
    if flattened_arg_felts < FPI_ABI_PREFIX_ARGS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` must pass account id and procedure root"
        )));
    }
    if procedure_input_count > FPI_EXEC_INPUTS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` passes {procedure_input_count} flattened procedure \
             input felts, but `execute_foreign_procedure` supports at most {FPI_EXEC_INPUTS}"
        )));
    }
    let fpi_result_count = fpi_flat_value_count(&flattened_results)?;
    if fpi_result_count > FPI_EXEC_RESULTS {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI import `{core_func_path}` returns {fpi_result_count} result felts, but \
             `execute_foreign_procedure` supports at most {FPI_EXEC_RESULTS}"
        )));
    }

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

    Ok(FpiCallShape {
        has_arg_ptr,
        has_output_ptr,
        flattened_params,
        flattened_results,
        flattened_arg_felts,
    })
}

/// Emits the felt-only protocol argument list for a validated FPI call shape.
fn lower_fpi_canonical_args(
    shape: &FpiCallShape,
    import_func_ty: &ComponentFunctionType,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<LoweredFpiAbi> {
    let output_ptr = if shape.has_output_ptr {
        Some(*args.last().ok_or_else(|| {
            midenc_session::diagnostics::Report::msg(
                "FPI import with an output pointer did not receive an output pointer argument",
            )
        })?)
    } else {
        None
    };
    let fpi_args = if shape.has_arg_ptr {
        let arg_ptr = *args.first().ok_or_else(|| {
            midenc_session::diagnostics::Report::msg(
                "FPI import exceeding the direct parameter budget did not receive an argument \
                 pointer",
            )
        })?;
        lower_fpi_indirect_args(fb, arg_ptr, &import_func_ty.ir.params, span)?
    } else {
        // Arguments cross the FPI boundary as bare felts, so variant discriminants must be
        // range-checked here, mirroring the direct component wrappers. The indirect path
        // validates during the canonical loads instead.
        let flat_args = &args[..shape.flattened_params.len()];
        validate_flat_variants(fb, &import_func_ty.ir.params, flat_args, span)?;
        lower_fpi_direct_args(fb, &shape.flattened_params, flat_args, span)?
    };

    if fpi_args.len() != shape.flattened_arg_felts {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI lowering produced {} argument felts, but the validated call shape expects {}",
            fpi_args.len(),
            shape.flattened_arg_felts
        )));
    }

    Ok(LoweredFpiAbi {
        fpi_args,
        output_ptr,
    })
}

/// Converts canonical flat direct FPI arguments to the felt-only protocol argument list.
fn lower_fpi_direct_args(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    flat_params: &[AbiParam],
    canonical_args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    if flat_params.len() != canonical_args.len() {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "FPI argument lowering expected {} canonical values, but received {}",
            flat_params.len(),
            canonical_args.len()
        )));
    }

    let mut fpi_args = Vec::with_capacity(fpi_flat_value_count(flat_params)?);
    for (param, arg) in flat_params.iter().zip(canonical_args) {
        push_fpi_arg_felts(fb, &param.ty, *arg, &mut fpi_args, span)?;
    }
    Ok(fpi_args)
}

/// Loads canonical arguments, retaining their typed carriers and canonical tuple layout.
/// Recursive loading validates variants and zero-fills unused joined payload positions.
fn load_canonical_args(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    arg_ptr: ValueRef,
    params: &[Type],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let mut args = Vec::new();
    let mut offset = 0u32;
    for param in params {
        let param_offset = canonical_abi_info(param)?.next_field32(&mut offset);
        let param_addr = offset_addr(fb, arg_ptr, param_offset, span)?;
        let mut values = SmallVec::<[ValueRef; 8]>::new();
        load(fb, param_addr, param, &mut values, span)?;
        let flat_types = canonical_flat_types(param)?;
        if values.len() != flat_types.len() {
            return Err(Report::msg(format!(
                "canonical argument load for `{param}` produced {} values, but its flat shape has \
                 {}",
                values.len(),
                flat_types.len()
            )));
        }
        args.extend(values);
    }
    Ok(args)
}

/// Expands typed canonical tuple values to the felt-only FPI protocol representation.
fn lower_fpi_indirect_args(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    arg_ptr: ValueRef,
    params: &[Type],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let args = load_canonical_args(fb, arg_ptr, params, span)?;
    let context = HirOpBuilder::builder(fb).context_rc();
    let flat_params = flatten_types(&context, params)?;
    lower_fpi_direct_args(fb, &flat_params, &args, span)
}

/// Appends the FPI felt representation for one canonical ABI flat value.
fn push_fpi_arg_felts(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    ty: &Type,
    arg: ValueRef,
    fpi_args: &mut Vec<ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
    match ty {
        // A 64-bit integer crosses as it sits on the operand stack: its low limb first
        Type::I64 | Type::U64 => {
            let (high, low) = fb.split2(arg, Type::Felt, span)?;
            fpi_args.push(low);
            fpi_args.push(high);
        }
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::Felt => {
            fpi_args.push(canonical_arg_to_felt(fb, arg, span)?);
        }
        other => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "unsupported flattened FPI argument type `{other}`"
            )));
        }
    }

    Ok(())
}

/// Converts FPI result felts to canonical flat result values.
fn lower_fpi_result_felts(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    flat_results: &[AbiParam],
    fpi_results: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let mut results = Vec::with_capacity(flat_results.len());
    let mut next_result = 0;
    for result in flat_results {
        push_fpi_result_value(fb, &result.ty, fpi_results, &mut next_result, &mut results, span)?;
    }
    Ok(results)
}

/// Appends one canonical ABI flat value decoded from FPI result felts.
fn push_fpi_result_value(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    ty: &Type,
    fpi_results: &[ValueRef],
    next_result: &mut usize,
    results: &mut Vec<ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
    match ty {
        // A 64-bit integer crosses as it sits on the operand stack: its low limb first
        Type::I64 | Type::U64 => {
            let low = take_value(fpi_results, next_result, "FPI result")?;
            let high = take_value(fpi_results, next_result, "FPI result")?;
            results.push(fb.join2(high, low, Type::I64, span)?);
        }
        Type::I1 | Type::I8 | Type::U8 | Type::I16 | Type::U16 | Type::I32 | Type::U32 => {
            let result = take_value(fpi_results, next_result, "FPI result")?;
            results.push(fb.bitcast(result, Type::I32, span)?);
        }
        Type::Felt => {
            results.push(take_value(fpi_results, next_result, "FPI result")?);
        }
        other => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "unsupported flattened FPI result type `{other}`"
            )));
        }
    }

    Ok(())
}

/// Returns how many protocol felts are needed to represent canonical ABI flat values across FPI.
fn fpi_flat_value_count(flat_values: &[AbiParam]) -> WasmResult<usize> {
    flat_values
        .iter()
        .try_fold(0usize, |count, param| Ok(count + fpi_flat_type_felt_count(&param.ty)?))
}

/// Returns how many protocol felts are needed for one canonical ABI flat type across FPI.
fn fpi_flat_type_felt_count(ty: &Type) -> WasmResult<usize> {
    Ok(match ty {
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::Felt => 1,
        Type::I64 | Type::U64 => 2,
        other => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "unsupported flattened FPI value type `{other}`"
            )));
        }
    })
}

fn take_value(values: &[ValueRef], next: &mut usize, label: &str) -> WasmResult<ValueRef> {
    let value = values.get(*next).copied().ok_or_else(|| {
        midenc_session::diagnostics::Report::msg(format!(
            "{label} lowering expected another value at index {}",
            *next
        ))
    })?;
    *next += 1;
    Ok(value)
}

fn canonical_arg_to_felt(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    arg: ValueRef,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    if arg.borrow().ty() == &Type::Felt {
        Ok(arg)
    } else {
        Ok(fb.bitcast(arg, Type::Felt, span)?)
    }
}

/// Returns true for WIT imports generated for foreign procedure invocation.
fn is_fpi_import(import_func_path: &SymbolPath, import_func_ty: &FunctionType) -> WasmResult<bool> {
    if !import_func_path.name().as_str().starts_with(FPI_IMPORT_PREFIX) {
        return Ok(false);
    }

    validate_fpi_import_shape(import_func_path, import_func_ty)?;
    Ok(true)
}

/// Validates that an import with the reserved FPI prefix has the generated FPI ABI prefix.
fn validate_fpi_import_shape(
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
) -> WasmResult<()> {
    let params = import_func_ty.params.as_slice();
    let valid_shape = has_structured_fpi_abi_prefix(params) || has_flattened_fpi_abi_prefix(params);
    if !valid_shape {
        return Err(midenc_session::diagnostics::Report::msg(format!(
            "import `{import_func_path}` uses reserved FPI prefix `{FPI_IMPORT_PREFIX}` but does \
             not have the generated FPI ABI prefix `felt, felt, word`"
        )));
    }

    Ok(())
}

/// Returns true when `params` begin with the generated `felt, felt, word` prefix.
fn has_structured_fpi_abi_prefix(params: &[Type]) -> bool {
    matches!(
        params,
        [account_id_prefix, account_id_suffix, proc_root, ..]
            if is_felt_type(account_id_prefix)
                && is_felt_type(account_id_suffix)
                && is_proc_root_word_type(proc_root)
    )
}

/// Returns true when `params` begin with a flattened generated `felt, felt, word` prefix.
fn has_flattened_fpi_abi_prefix(params: &[Type]) -> bool {
    matches!(
        params,
        [
            account_id_prefix,
            account_id_suffix,
            proc_root_a,
            proc_root_b,
            proc_root_c,
            proc_root_d,
            ..
        ] if [
            account_id_prefix,
            account_id_suffix,
            proc_root_a,
            proc_root_b,
            proc_root_c,
            proc_root_d,
        ]
        .into_iter()
        .all(is_felt_type)
    )
}

/// Returns true when `ty` matches the generated `felt` type, bare or wrapped in the WIT
/// `core-types` record.
fn is_felt_type(ty: &Type) -> bool {
    match ty {
        Type::Felt => true,
        Type::Struct(struct_ty) => {
            matches!(struct_ty.get().fields(), [field] if field.offset == 0 && field.ty == Type::Felt)
        }
        _ => false,
    }
}

/// Returns true when `ty` matches the generated procedure-root `word` record, i.e. the record of
/// four `felt` fields both FPI and dyncall imports carry their callee's MAST root in.
fn is_proc_root_word_type(ty: &Type) -> bool {
    let Type::Struct(struct_ty) = ty else {
        return false;
    };
    let struct_ty = struct_ty.get();
    struct_ty.fields().len() == 4 && struct_ty.fields().iter().all(|field| is_felt_type(&field.ty))
}

/// Returns true when the import is a synthesized `dyncall-` import, validating its shape.
fn is_dyncall_import(
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
) -> WasmResult<bool> {
    if !import_func_path.name().as_str().starts_with(DYNCALL_IMPORT_PREFIX) {
        return Ok(false);
    }

    validate_dyncall_import_shape(import_func_path, import_func_ty)?;
    Ok(true)
}

/// Validates that an import with the reserved `dyncall-` prefix leads with the procedure-root
/// `word` parameter.
///
/// Unlike its FPI counterpart [`validate_fpi_import_shape`], which also accepts its generated
/// `felt, felt, word` prefix already flattened to bare felts, this check rejects a root spelled
/// as four felts, because the dyncall path can never see one:
///
/// * The only producer of `dyncall-` imports is `#[component_storage]`, whose generated
///   stored-procedure world declares every import with a leading `proc-root: word` parameter.
///   The SDK macros reject the prefix in every other world, so no user-authored or dependency
///   WIT reaches here with it.
/// * [`ComponentFunctionType::from_component_type`] maps a WIT record to a struct type, so that
///   `word` is still a single struct parameter at this point; it is expanded into four felts only
///   later, by the canonical flattening in [`split_dyncall_root`].
///
/// Four leading bare felts therefore mean a hand-written interface misusing the reserved prefix,
/// which is what the diagnostic reports.
fn validate_dyncall_import_shape(
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
) -> WasmResult<()> {
    if !leads_with_proc_root_word(&import_func_ty.params) {
        return Err(Report::msg(format!(
            "import `{import_func_path}` uses reserved prefix `{DYNCALL_IMPORT_PREFIX}` but does \
             not lead with the procedure-root `word` parameter"
        )));
    }

    Ok(())
}

/// Returns true when `params` lead with the procedure-root `word` record of a dyncall import.
///
/// A root already flattened to four bare felts is deliberately not accepted, see
/// [`validate_dyncall_import_shape`].
fn leads_with_proc_root_word(params: &[Type]) -> bool {
    matches!(params, [proc_root, ..] if is_proc_root_word_type(proc_root))
}

/// Splits a dyncall import's leading procedure-root parameter off its IR type.
///
/// Returns the payload type and the root's four flat parameters. The payload type is used only
/// for actual target budgeting; canonical source classification retains the complete root.
fn split_dyncall_root(
    context: &Rc<Context>,
    import_func_path: &SymbolPath,
    import_func_ty: &FunctionType,
) -> WasmResult<(FunctionType, Vec<AbiParam>)> {
    assert!(
        leads_with_proc_root_word(&import_func_ty.params),
        "dyncall import shape validation guarantees a leading procedure-root `word` parameter"
    );
    let (root_tys, arg_tys) = import_func_ty.params.split_at(1);
    let root_flat_params = flatten_types(context, root_tys).wrap_err_with(|| {
        format!("failed to flatten the procedure-root parameter of '{import_func_path}'")
    })?;
    let mut arguments_ty = import_func_ty.clone();
    arguments_ty.params = arg_tys.iter().cloned().collect();
    Ok((arguments_ty, root_flat_params))
}

/// Checks actual target arguments and results independently of canonical source indirection.
fn validate_import_target_capacity(
    context: &Rc<Context>,
    import_func_path: &SymbolPath,
    target_ty: &FunctionType,
    call_kind: ImportCallKind,
) -> WasmResult<()> {
    for (kind, types) in [
        ("argument", target_ty.params.as_slice()),
        ("result", target_ty.results.as_slice()),
    ] {
        let felts: usize =
            flatten_types(context, types)?.iter().map(|p| p.ty.size_in_felts()).sum();
        if felts > MAX_DIRECT_STACK_FELTS {
            let label = match call_kind {
                ImportCallKind::Call => "component import",
                ImportCallKind::Dyncall => "stored procedure",
            };
            return Err(Report::msg(format!(
                "unsupported {label} signature for '{import_func_path}': {felts} {kind} field \
                 elements exceed the {MAX_DIRECT_STACK_FELTS}-element context {kind} ABI"
            )));
        }
    }
    Ok(())
}

/// Emits the call from a lowering function to its import and returns the call's results.
///
/// For [ImportCallKind::Call] the import is declared at its Miden path (see
/// [`declare_import_function`]) and reached with `hir.call`, to be resolved by path at assembly
/// time. For
/// [ImportCallKind::Dyncall] the leading four flat arguments are the callee's MAST root and the
/// rest its arguments, reached with `hir.dyncall` under the signature with the root removed;
/// nothing is declared, as the target is runtime data.
fn build_import_call(
    world_builder: &mut WorldBuilder,
    call_kind: ImportCallKind,
    import: &ComponentImportPath,
    import_func_sig: Signature,
    args: Vec<ValueRef>,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let results = match call_kind {
        ImportCallKind::Call => {
            let import_func_ref = declare_import_function(world_builder, import, &import_func_sig)?;
            let call = fb.call(import_func_ref, import_func_sig, args, span)?;
            let call = call.borrow();
            call.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
        }
        ImportCallKind::Dyncall => {
            assert!(
                args.len() >= DYNCALL_ROOT_FELTS
                    && import_func_sig.params.len() >= DYNCALL_ROOT_FELTS,
                "dyncall import `{}` lost its procedure-root parameter",
                import.cm_path
            );
            let (root, call_args) = args.split_at(DYNCALL_ROOT_FELTS);
            let root: [ValueRef; DYNCALL_ROOT_FELTS] = root.try_into().unwrap();
            let signature = Signature {
                params: import_func_sig.params[DYNCALL_ROOT_FELTS..].to_vec(),
                results: import_func_sig.results,
                cc: import_func_sig.cc,
            };

            // Only the actual callee arguments consume the context-call window. The root
            // and wrapper result pointer have separate storage and source-call obligations.
            let call = fb.dyncall(root, signature, call_args.to_vec(), span)?;
            let call = call.borrow();
            call.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
        }
    };
    Ok(results)
}

/// Rejects component import signatures that require tuple-parameter lowering.
fn reject_tuple_parameter_import_lowering<T>(import_func_path: &SymbolPath) -> WasmResult<T> {
    Err(Report::msg(format!(
        "tuple-parameter import lowering is not supported for '{import_func_path}'"
    )))
}

/// Normalizes the complete canonical source arguments before constructing the target call.
/// Tuple and output pointers belong to this same-context wrapper, never to the target payload.
#[allow(clippy::too_many_arguments)]
fn generate_lowering(
    world_builder: &mut WorldBuilder,
    call_kind: ImportCallKind,
    import: &ComponentImportPath,
    import_func_ty: &ComponentFunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    transformation: CanonicalAbiIndirection,
    core_func_ref: FunctionRef,
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    args: &[ValueRef],
    span: SourceSpan,
) -> WasmResult<CallableFunction> {
    let has_output_ptr =
        matches!(transformation, CanonicalAbiIndirection::Out | CanonicalAbiIndirection::InOut);
    // Exact core source equivalence was checked before creating the wrapper or reading arguments.
    let output_ptr = has_output_ptr.then(|| *args.last().expect("validated source output pointer"));
    let canonical_args = if transformation.has_param_tuple() {
        load_canonical_args(fb, args[0], &import_func_ty.ir.params, span)?
    } else {
        let inputs = &args[..args.len() - usize::from(has_output_ptr)];
        validate_flat_variants(fb, &import_func_ty.ir.params, inputs, span)?;
        inputs.to_vec()
    };
    let context = world_builder.context_rc();
    let target_sig = Signature {
        params: flatten_types(&context, &import_func_ty.ir.params)?,
        results: flatten_types(&context, &import_func_ty.ir.results)?,
        cc: midenc_hir::CallConv::ComponentModel,
    };
    let results =
        build_import_call(world_builder, call_kind, import, target_sig, canonical_args, fb, span)?;
    validate_flat_variants(fb, &import_func_ty.ir.results, &results, span)?;
    if let Some(output_ptr) = output_ptr {
        assert_eq!(import_func_ty.ir.results.len(), 1, "canonical source has one result type");
        store(fb, output_ptr, &import_func_ty.ir.results[0], &mut results.into_iter(), span)?;
        fb.ret([], span)?;
    } else {
        fb.ret(results, span)?;
    }
    Ok(CallableFunction::Function {
        wasm_id: core_func_path,
        function_ref: core_func_ref,
        signature: core_func_sig,
    })
}

/// Declares the imported function at the Miden path `import.path` with `signature` in the stub
/// component named by the path's parent.
///
/// When an earlier import already declared that path, its declaration is returned if the
/// signatures agree, and an error is reported otherwise; a new declaration records the import's
/// component-model path for that diagnostic. An import path inside the namespace of
/// the component being translated is rejected: its stub component would nest in that namespace.
///
/// Only [`ImportCallKind::Call`] imports reach this function. FPI imports
/// ([`generate_fpi_lowering`]) and dyncall imports (the [`ImportCallKind::Dyncall`] arm of
/// [`build_import_call`]) reach their callee by its procedure root and declare nothing, which is
/// why their paths may nest in the component's own namespace (`<namespace>::fpi::...`,
/// `<namespace>::dyncall::...`).
fn declare_import_function(
    world_builder: &mut WorldBuilder,
    import: &ComponentImportPath,
    signature: &Signature,
) -> WasmResult<FunctionRef> {
    let import_path = &import.path;
    let component_name = import_path.without_leaf().to_symbol_name()?;
    let inside_namespace = component_name == import.namespace
        || SymbolPath::nests_in(component_name, import.namespace);
    if inside_namespace {
        let namespace = SymbolPath::from_symbol_name(import.namespace);
        return Err(Report::msg(format!(
            "import `{import_path}` lies inside this component's own namespace `{namespace}`"
        )));
    }
    let component_ref = match world_builder.find_component(component_name) {
        Some(component_ref) => component_ref,
        None => world_builder.define_component(Ident::with_empty_span(component_name))?,
    };
    let existing = component_ref.borrow().get(import_path.name());
    if let Some(existing) = existing {
        let existing = existing.borrow();
        let Some(function) = existing.as_symbol_operation().downcast_ref::<Function>() else {
            return Err(Report::msg(format!(
                "`{import_path}` is already declared as a non-function symbol"
            )));
        };
        if *function.get_signature() == *signature {
            return Ok(function.as_function_ref());
        }
        let first_cm_path = function
            .as_operation()
            .get_typed_attribute::<StringAttr>(COMPONENT_IMPORT_PATH_ATTR)
            .map(|attr| (**attr.borrow()).clone())
            .filter(|first| first.as_str() != import.cm_path.to_string());
        return Err(Report::msg(match first_cm_path {
            Some(first) => format!(
                "imports `{first}` and `{}` both lower to `{import_path}` with different \
                 signatures",
                import.cm_path
            ),
            // Declared by an earlier translation into the same world, or not by an import
            None => format!(
                "import `{}` lowers to `{import_path}`, which is already declared with a \
                 different signature",
                import.cm_path
            ),
        }));
    }
    let context = world_builder.context_rc();
    let mut function_ref = ComponentBuilder::new(component_ref).define_function(
        import_path.name().into(),
        Visibility::Internal,
        signature.clone(),
    )?;
    let cm_path = context.create_attribute::<StringAttr, _>(import.cm_path.to_string());
    function_ref.borrow_mut().set_attribute(COMPONENT_IMPORT_PATH_ATTR, cm_path);
    Ok(function_ref)
}

/// Rejects component import signatures containing unsupported canonical ABI shapes.
fn reject_unsupported_import_canonical_abi_types(
    import_func_path: &SymbolPath,
    import_func_ty: &ComponentFunctionType,
) -> WasmResult<()> {
    for ty in import_func_ty.ir.params.iter().chain(import_func_ty.ir.results.iter()) {
        if contains_unsupported_canonical_abi_type(ty) {
            return Err(Report::msg(format!(
                "component import lowering for '{import_func_path}' has unsupported canonical ABI \
                 type {:?}",
                ty
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use midenc_hir::{
        CallConv, Context, FunctionType, PointerType, StructType, SymbolName, SymbolNameComponent,
        SymbolPath, Type,
        dialects::builtin::{FunctionRef, attributes::AbiParam},
        interner::Symbol,
    };

    use super::*;
    use crate::component::{
        MAX_FLAT_PARAMS,
        test_support::{
            count_ops, count_validation_ops, mixed_payload_variant_type,
            pointer_payload_variant_type, scalar_payload_variant_type, two_field_record_type,
            unit_only_variant_type, world_with_core_module,
        },
    };

    fn test_import_path(name: &str) -> SymbolPath {
        SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(Symbol::intern("miden")),
            SymbolNameComponent::Component(Symbol::intern("counter-account")),
            SymbolNameComponent::Leaf(Symbol::intern(name)),
        ])
    }

    fn word_type() -> Type {
        Type::from(StructType::new(vec![Type::Felt; 4]))
    }

    fn wit_felt_type() -> Type {
        Type::from(StructType::named(
            Arc::from("miden:base/core-types@2.0.0/felt"),
            [(Arc::from("inner"), Type::Felt)],
        ))
    }

    fn wit_word_type() -> Type {
        let felt_ty = wit_felt_type();
        Type::from(StructType::named(
            Arc::from("miden:base/core-types@2.0.0/word"),
            [
                (Arc::from("a"), felt_ty.clone()),
                (Arc::from("b"), felt_ty.clone()),
                (Arc::from("c"), felt_ty.clone()),
                (Arc::from("d"), felt_ty),
            ],
        ))
    }

    fn fpi_params_with_user_args(user_args: impl IntoIterator<Item = Type>) -> Vec<Type> {
        (0..FPI_ABI_PREFIX_ARGS).map(|_| Type::Felt).chain(user_args).collect()
    }

    #[test]
    fn is_fpi_import_ignores_non_prefixed_imports() {
        let import_func_ty = FunctionType::new(CallConv::Wasm, vec![Type::Felt], vec![Type::Felt]);
        let import_func_path = test_import_path("get-count");

        let is_fpi = is_fpi_import(&import_func_path, &import_func_ty)
            .expect("non-prefixed imports should not validate the FPI ABI shape");

        assert!(!is_fpi);
    }

    #[test]
    fn is_fpi_import_accepts_generated_abi_prefix() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![Type::Felt, Type::Felt, word_type(), Type::Felt],
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-get-count");

        let is_fpi = is_fpi_import(&import_func_path, &import_func_ty)
            .expect("generated FPI imports should pass the ABI shape check");

        assert!(is_fpi);
    }

    #[test]
    fn is_fpi_import_accepts_wrapped_generated_abi_prefix() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![wit_felt_type(), wit_felt_type(), wit_word_type()],
            vec![wit_felt_type()],
        );
        let import_func_path = test_import_path("fpi-get-count");

        let is_fpi = is_fpi_import(&import_func_path, &import_func_ty)
            .expect("generated FPI imports should accept the WIT core-types wrappers");

        assert!(is_fpi);
    }

    #[test]
    fn is_fpi_import_accepts_flattened_generated_abi_prefix() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::Felt,
                Type::U32,
            ],
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-get-count");

        let is_fpi = is_fpi_import(&import_func_path, &import_func_ty)
            .expect("generated FPI imports may reach lowering with the word prefix flattened");

        assert!(is_fpi);
    }

    #[test]
    fn is_fpi_import_rejects_reserved_prefix_without_generated_abi() {
        let import_func_ty =
            FunctionType::new(CallConv::Wasm, vec![Type::Felt, Type::Felt], vec![Type::Felt]);
        let import_func_path = test_import_path("fpi-ordinary-import");

        let err = is_fpi_import(&import_func_path, &import_func_ty)
            .expect_err("reserved FPI prefix without the generated ABI must be rejected");
        let message = err.to_string();

        assert!(
            message.contains("reserved FPI prefix `fpi-`")
                && message.contains("generated FPI ABI prefix `felt, felt, word`"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn is_dyncall_import_ignores_non_prefixed_imports() {
        let import_func_ty =
            FunctionType::new(CallConv::Wasm, vec![word_type(), Type::Felt], vec![Type::Felt]);
        let import_func_path = test_import_path("get-count");

        let is_dyncall = is_dyncall_import(&import_func_path, &import_func_ty)
            .expect("non-prefixed imports should not validate the dyncall shape");

        assert!(!is_dyncall);
    }

    #[test]
    fn is_dyncall_import_accepts_leading_word_record() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![wit_word_type(), wit_felt_type()],
            vec![wit_felt_type()],
        );
        let import_func_path = test_import_path("dyncall-authority");

        let is_dyncall = is_dyncall_import(&import_func_path, &import_func_ty)
            .expect("generated dyncall imports should accept the WIT core-types word");

        assert!(is_dyncall);
    }

    #[test]
    fn is_dyncall_import_rejects_four_leading_bare_felts() {
        // The generated import always spells the root as the `word` record, so four leading
        // felts are a hand-written signature misusing the reserved prefix, not a flattened root
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            vec![Type::Felt, Type::Felt, Type::Felt, Type::Felt, Type::U32],
            vec![],
        );
        let import_func_path = test_import_path("dyncall-authority");

        let err = is_dyncall_import(&import_func_path, &import_func_ty)
            .expect_err("a flattened leading word must not be taken for a procedure root");
        let message = err.to_string();

        assert!(
            message.contains("reserved prefix `dyncall-`")
                && message.contains("procedure-root `word` parameter"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn is_dyncall_import_rejects_reserved_prefix_without_leading_word() {
        let import_func_ty =
            FunctionType::new(CallConv::Wasm, vec![Type::Felt, Type::U32], vec![Type::Felt]);
        let import_func_path = test_import_path("dyncall-ordinary-import");

        let err = is_dyncall_import(&import_func_path, &import_func_ty)
            .expect_err("reserved dyncall prefix without the leading word must be rejected");
        let message = err.to_string();

        assert!(
            message.contains("reserved prefix `dyncall-`")
                && message.contains("procedure-root `word` parameter"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_list_param_on_direct_path() {
        let context = Rc::new(Context::default());
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args([Type::List(Arc::new(Type::U32))]),
            vec![Type::Felt],
        );
        let flattened_params = flatten_types(&context, &import_func_ty.params).unwrap();
        let import_func_path = test_import_path("fpi-read-list");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("list parameters must not lower directly across typed FPI");
        let message = err.to_string();

        assert!(flattened_params.len() <= MAX_FLAT_PARAMS);
        assert!(
            message.contains("parameter 6")
                && message.contains("pointer-like type")
                && message.contains("list, string, or pointer values"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_string_like_list_in_indirect_arg() {
        let context = Rc::new(Context::default());
        let string_like_list = Type::List(Arc::new(Type::U8));
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args((0..15).map(|_| Type::U32).chain([string_like_list])),
            vec![Type::Felt],
        );
        let flattened_params = flatten_types(&context, &import_func_ty.params).unwrap();
        let import_func_path = test_import_path("fpi-read-string-like-list");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("string-like list values must not lower indirectly across typed FPI");
        let message = err.to_string();

        assert!(flattened_params.len() > MAX_FLAT_PARAMS);
        assert!(
            message.contains("parameter 21")
                && message.contains("pointer-like type")
                && message.contains("caller linear-memory addresses"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_pointer_inside_struct_param() {
        let pointer_struct = Type::from(StructType::new([(
            Arc::from("ptr"),
            Type::from(PointerType::new(Type::U8)),
        )]));
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args([pointer_struct]),
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-read-pointer-struct");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("pointer fields inside aggregate parameters must be rejected");
        let message = err.to_string();

        assert!(
            message.contains("parameter 6.ptr")
                && message.contains("pointer-like type")
                && message.contains("list, string, or pointer values"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_unsupported_direct_param_types() {
        let unsupported_types = vec![
            Type::Unknown,
            Type::Never,
            Type::I128,
            Type::U128,
            Type::U256,
            Type::F64,
            Type::from(FunctionType::new(CallConv::Wasm, vec![], vec![])),
        ];

        for (index, unsupported_ty) in unsupported_types.into_iter().enumerate() {
            let import_func_ty = FunctionType::new(
                CallConv::Wasm,
                fpi_params_with_user_args([unsupported_ty]),
                vec![Type::Felt],
            );
            let import_func_path = test_import_path(&format!("fpi-read-unsupported-{index}"));

            let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
                .expect_err("unsupported FPI parameter types must be rejected before flattening");
            let message = err.to_string();

            assert!(
                message.contains("parameter 6")
                    && message.contains("unsupported type")
                    && message.contains("typed FPI only supports"),
                "unexpected error: {message}"
            );
        }
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_unsupported_indirect_param_type() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args((0..15).map(|_| Type::U32).chain([Type::U128])),
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-read-unsupported-indirect");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty).expect_err(
            "unsupported indirect FPI parameter types must be rejected before flattening",
        );
        let message = err.to_string();

        assert!(
            message.contains("parameter 21")
                && message.contains("unsupported type")
                && message.contains("typed FPI only supports"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_accepts_payload_enum_param() {
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args([scalar_payload_variant_type()]),
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-read-payload-enum");

        validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect("payload-carrying enum parameters are supported over typed FPI");
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_pointer_inside_enum_payload() {
        let enum_ty = pointer_payload_variant_type();
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args([enum_ty]),
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-read-pointer-payload-enum");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("pointer-like enum payloads must be rejected before flattening");
        let message = err.to_string();

        assert!(
            message.contains("parameter 6::items") && message.contains("pointer-like type"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn validate_fpi_typed_signature_rejects_unsupported_result_type() {
        let import_func_ty =
            FunctionType::new(CallConv::Wasm, fpi_params_with_user_args([]), vec![Type::U256]);
        let import_func_path = test_import_path("fpi-return-unsupported");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("unsupported FPI result types must be rejected before flattening");
        let message = err.to_string();

        assert!(
            message.contains("result 0")
                && message.contains("unsupported type")
                && message.contains("typed FPI only supports"),
            "unexpected error: {message}"
        );
    }

    /// Plans an FPI call shape from the typed import signature, using the canonical ABI lowered
    /// signature as both the import and core function signatures, mirroring the real pipeline.
    fn plan_fpi_call_for(
        context: &Rc<Context>,
        import_func_ty: &FunctionType,
        core_func_name: &str,
    ) -> WasmResult<FpiCallShape> {
        let import_lowered_sig =
            flatten_function_type(context, import_func_ty, CanonicalAbiMode::Import).unwrap();
        let core_func_sig = expected_core_signature(&import_lowered_sig);
        plan_fpi_call(
            context,
            import_func_ty,
            &import_lowered_sig,
            &test_import_path(core_func_name),
            &core_func_sig,
            core_func_sig.params().len(),
        )
    }

    #[test]
    fn plan_fpi_call_rejects_mismatched_core_param_types() {
        let context = Rc::new(Context::default());
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([Type::U32]),
            vec![Type::Felt],
        );
        let import_lowered_sig =
            flatten_function_type(&context, &import_func_ty, CanonicalAbiMode::Import).unwrap();
        // The user argument lowers to a core `i32`, but the core import declares `i64`.
        let mut core_func_sig = expected_core_signature(&import_lowered_sig);
        *core_func_sig.params.last_mut().unwrap() = AbiParam::new(Type::I64);

        let err = plan_fpi_call(
            &context,
            &import_func_ty,
            &import_lowered_sig,
            &test_import_path("fpi-mismatched-core-param"),
            &core_func_sig,
            core_func_sig.params().len(),
        )
        .expect_err("mismatched core parameter types must be rejected");
        let message = err.to_string();

        assert!(message.contains("core Wasm signature mismatch"), "unexpected error: {message}");
    }

    #[test]
    fn plan_fpi_call_rejects_too_many_procedure_inputs() {
        let context = Rc::new(Context::default());
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args((0..FPI_EXEC_INPUTS + 1).map(|_| Type::Felt)),
            vec![Type::Felt],
        );

        let err = plan_fpi_call_for(&context, &import_func_ty, "fpi-get-count-sum-by-keys")
            .expect_err(
                "expected FPI validation to reject more than sixteen procedure input felts",
            );
        let message = err.to_string();

        assert!(
            message.contains("passes 17 flattened procedure input felts")
                && message.contains("`execute_foreign_procedure` supports at most 16"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn plan_fpi_call_accepts_full_width_procedure_inputs() {
        let context = Rc::new(Context::default());
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args((0..FPI_EXEC_INPUTS).map(|_| Type::Felt)),
            vec![Type::Felt],
        );

        let shape = plan_fpi_call_for(&context, &import_func_ty, "fpi-get-count")
            .expect("sixteen procedure input felts are within the protocol limit");

        assert!(
            shape.has_arg_ptr,
            "22 flattened parameters must use the canonical tuple pointer"
        );
        assert_eq!(shape.flattened_arg_felts, FPI_ABI_PREFIX_ARGS + FPI_EXEC_INPUTS);
    }

    #[test]
    fn plan_fpi_call_rejects_too_many_results() {
        let context = Rc::new(Context::default());
        let wide_record =
            Type::from(StructType::new((0..FPI_EXEC_RESULTS + 1).map(|_| Type::Felt)));
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([]),
            vec![wide_record],
        );

        let err = plan_fpi_call_for(&context, &import_func_ty, "fpi-get-count-words")
            .expect_err("expected FPI validation to reject more than sixteen result felts");
        let message = err.to_string();

        assert!(
            message.contains("returns 17 result felts")
                && message.contains("`execute_foreign_procedure` supports at most 16"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn plan_fpi_call_preserves_thirteen_flat_nineteen_felt_source_shape() {
        let context = Rc::new(Context::default());
        // Six u64 values plus one felt expand to thirteen payload lanes and nineteen wrapper lanes.
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args((0..6).map(|_| Type::U64).chain([Type::Felt])),
            vec![Type::Felt],
        );
        let shape = plan_fpi_call_for(&context, &import_func_ty, "fpi-echo-six-u64-record")
            .expect("prefix lanes do not consume payload capacity");
        assert!(!shape.has_arg_ptr);
        assert_eq!(shape.flattened_arg_felts, 19);
        let flat =
            flatten_function_type(&context, &import_func_ty, CanonicalAbiMode::Import).unwrap();
        assert_eq!(flat.params().len(), 13);
        assert_eq!(expected_core_signature(&flat).params()[6].ty, Type::I64);
        let scalar_params = flatten_types(&context, &import_func_ty.params).unwrap();
        assert_eq!(scalar_params.len(), 13);
    }

    #[test]
    fn plan_fpi_call_counts_variant_payload_lanes() {
        let context = Rc::new(Context::default());
        // The mixed-width variant flattens to a discriminant lane plus one joined i64 payload
        // lane, i.e. three protocol felts.
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([mixed_payload_variant_type()]),
            vec![Type::Felt],
        );

        let shape = plan_fpi_call_for(&context, &import_func_ty, "fpi-send-mixed-variant")
            .expect("payload variant params fit the direct FPI shape");

        assert!(!shape.has_arg_ptr, "three variant lanes stay within the direct shape");
        assert_eq!(shape.flattened_arg_felts, FPI_ABI_PREFIX_ARGS + 3);
    }

    #[test]
    fn plan_fpi_call_routes_variant_results_through_output_pointer() {
        let context = Rc::new(Context::default());
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([]),
            vec![mixed_payload_variant_type()],
        );

        let shape = plan_fpi_call_for(&context, &import_func_ty, "fpi-get-mixed-variant")
            .expect("payload variant results are supported over typed FPI");

        assert!(
            shape.has_output_ptr,
            "multi-lane variant results must use the canonical output pointer"
        );
    }

    #[test]
    fn plan_fpi_call_counts_variant_lanes_against_procedure_input_limit() {
        let context = Rc::new(Context::default());
        // Nine u64 params plus the joined variant lanes only just cross the count-based
        // indirect threshold, but their felt expansion exceeds the executor's input window.
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args(
                (0..9).map(|_| Type::U64).chain([mixed_payload_variant_type()]),
            ),
            vec![Type::Felt],
        );

        let err = plan_fpi_call_for(&context, &import_func_ty, "fpi-send-wide-variant")
            .expect_err("variant payload lanes must count against the FPI input budget");
        let message = err.to_string();

        assert!(
            message.contains("passes 21 flattened procedure input felts")
                && message.contains("`execute_foreign_procedure` supports at most 16"),
            "unexpected error message: {message}"
        );
    }

    /// Pairs the component-model path `cm_path` with a Miden path under `::miden::test::test`.
    fn import_paths(cm_path: SymbolPath) -> ComponentImportPath {
        let leaf = cm_path.name().as_str().replace('-', "_");
        import_at(cm_path, &format!("miden::test::test::{leaf}"))
    }

    /// Pairs the component-model path `cm_path` with the Miden path `path`
    /// (`<module>::<function>`), imported by a component in the namespace `miden::test::app`.
    fn import_at(cm_path: SymbolPath, path: &str) -> ComponentImportPath {
        let (module, leaf) = path.rsplit_once("::").expect("a path with a module");
        let mut path = SymbolPath::from_masm_module_id(module);
        path.path.push(SymbolNameComponent::Leaf(SymbolName::intern(leaf)));
        ComponentImportPath {
            cm_path,
            path,
            namespace: SymbolName::intern("miden::test::app"),
        }
    }

    fn component_import_path(function: &str) -> ComponentImportPath {
        import_paths(SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("miden:test@1.0.0")),
            SymbolNameComponent::Leaf(SymbolName::intern(function)),
        ]))
    }

    fn core_function_path(function: &str) -> SymbolPath {
        SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("core")),
            SymbolNameComponent::Leaf(SymbolName::intern(function)),
        ])
    }

    fn scalar_u64_type() -> Type {
        Type::U64
    }

    #[test]
    fn rejects_import_lowering_with_tupled_params_and_no_result() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let mut ir = FunctionType::new(CallConv::Fast, vec![Type::I32; 17], vec![]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };

        let tuple = Type::from(StructType::new(vec![Type::I32; 17]));
        let core_func_sig = Signature {
            params: vec![AbiParam::sret(Type::from(PointerType::new(tuple)), &context)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("too_many_params"),
            &import_func_ty,
            core_function_path("too_many_params"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("expected tuple-parameter import lowering to be rejected"),
            Err(err) => {
                assert!(
                    err.to_string().contains("tuple-parameter import lowering"),
                    "unexpected diagnostic: {err}"
                );
            }
        }
    }

    #[test]
    fn ordinary_import_result_pointer_does_not_consume_target_payload() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = two_field_record_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![Type::I32; 16], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };

        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::I32); 17],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("too_many_params_with_result"),
            &import_func_ty,
            core_function_path("too_many_params_with_result"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        result.expect("sixteen target arguments plus a wrapper result pointer are supported");
    }

    /// Nine `u64` values overflow the window on their own, so the diagnostic does not blame the
    /// result pointer that comes after them.
    #[test]
    fn direct_import_whose_arguments_alone_overflow_is_not_blamed_on_its_pointer() {
        let err = lower_direct_import(
            "direct_nine_u64_result",
            wide_direct_import(9, 0, Some(two_field_record_type())),
        )
        .expect_err("an 18-felt argument list should be rejected");
        let err = err.to_string();
        assert!(
            err.contains(
                "component import `::\"miden:test@1.0.0\"::direct_nine_u64_result` takes 18 \
                 argument felts after expanding 64-bit values, but a cross-context call passes at \
                 most 16"
            ),
            "unexpected diagnostic: {err}"
        );
        assert!(!err.contains("result pointer"), "unexpected diagnostic: {err}");
    }

    #[test]
    fn transformed_import_lowering_validates_flat_variant_params() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let variant_ty = unit_only_variant_type();
        let result_ty = two_field_record_type();
        let mut ir =
            FunctionType::new(CallConv::Fast, vec![variant_ty.clone()], vec![result_ty.clone()]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::zext(Type::I32, &context), AbiParam::new(Type::I32)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("roundtrip"),
            &import_func_ty,
            core_function_path("roundtrip"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("import lowering should build");

        let (switch_count, unreachable_count) =
            count_validation_ops(lowered.function_ref().expect("expected function lowering"));
        assert_eq!(switch_count, 1, "transformed import params should validate the tag");
        assert_eq!(
            unreachable_count, 1,
            "invalid transformed import param tag should be unreachable"
        );
    }

    #[test]
    fn transformed_import_lowering_validates_flat_variant_results_before_store() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = scalar_payload_variant_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("variant_result"),
            &import_func_ty,
            core_function_path("variant_result"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("import lowering should build");

        let (switch_count, unreachable_count) =
            count_validation_ops(lowered.function_ref().expect("expected function lowering"));
        assert_eq!(
            switch_count, 2,
            "transformed import results should validate the tag before storing"
        );
        assert_eq!(
            unreachable_count, 2,
            "invalid transformed import result tag should be unreachable before storing"
        );
    }

    #[test]
    fn fpi_direct_wrapper_preserves_executor_prefix_and_payload_order() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();
        let import_func_ty = ComponentFunctionType {
            ir: FunctionType::new(
                CallConv::ComponentModel,
                fpi_params_with_user_args([Type::Felt]),
                vec![Type::Felt],
            ),
        };
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::Felt); 7],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::Wasm,
        };
        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-order")),
            &import_func_ty,
            core_function_path("fpi-order"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("FPI wrapper builds");
        let function = lowered.function_ref().unwrap().borrow();
        let args = function
            .entry_block()
            .borrow()
            .arguments()
            .iter()
            .map(|arg| *arg as ValueRef)
            .collect::<Vec<_>>();
        let mut count = 0;
        function.as_operation().postwalk_all(|op| {
            assert!(op.downcast_ref::<midenc_dialect_hir::StoreLocal>().is_none());
            if let Some(exec) = op.downcast_ref::<ExecFpi>() {
                count += 1;
                let prefix = exec
                    .prefix()
                    .iter()
                    .map(|operand| operand.borrow().as_value_ref())
                    .collect::<Vec<_>>();
                assert_eq!(prefix, [args[1], args[0], args[2], args[3], args[4], args[5]]);
                assert_eq!(exec.inputs()[0].borrow().as_value_ref(), args[6]);
            }
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn fpi_direct_lowering_validates_variant_param_discriminant() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([scalar_payload_variant_type()]),
            vec![Type::Felt],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let mut core_params = vec![AbiParam::new(Type::Felt); FPI_ABI_PREFIX_ARGS];
        core_params.extend([AbiParam::new(Type::I32), AbiParam::new(Type::I32)]);
        let core_func_sig = Signature {
            params: core_params,
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::Wasm,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-send-variant")),
            &import_func_ty,
            core_function_path("fpi-send-variant"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("FPI import lowering should build");

        let (switch_count, unreachable_count) =
            count_validation_ops(lowered.function_ref().expect("expected function lowering"));
        assert_eq!(switch_count, 1, "direct FPI variant params should validate the tag");
        assert_eq!(unreachable_count, 1, "invalid direct FPI variant tag should be unreachable");
    }

    #[test]
    fn fpi_indirect_lowering_validates_variant_param_discriminant() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        // Nine u32 params plus the variant push the flat count past the indirect threshold, so
        // the wrapper reloads every argument from the canonical tuple pointer.
        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args(
                (0..9).map(|_| Type::U32).chain([scalar_payload_variant_type()]),
            ),
            vec![Type::Felt],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::I32)],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::Wasm,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-send-variant-indirect")),
            &import_func_ty,
            core_function_path("fpi-send-variant-indirect"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("FPI import lowering should build");

        let (switch_count, unreachable_count) =
            count_validation_ops(lowered.function_ref().expect("expected function lowering"));
        assert_eq!(switch_count, 1, "indirect FPI variant args should switch on the loaded tag");
        assert_eq!(unreachable_count, 1, "invalid indirect FPI variant tag should be unreachable");
    }

    #[test]
    fn fpi_lowering_validates_variant_result_before_store() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([]),
            vec![scalar_payload_variant_type()],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let mut core_params = vec![AbiParam::new(Type::Felt); FPI_ABI_PREFIX_ARGS];
        core_params.push(AbiParam::new(Type::I32));
        let core_func_sig = Signature {
            params: core_params,
            results: vec![],
            cc: CallConv::Wasm,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-get-variant")),
            &import_func_ty,
            core_function_path("fpi-get-variant"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("FPI import lowering should build");

        let (switch_count, unreachable_count) =
            count_validation_ops(lowered.function_ref().expect("expected function lowering"));
        assert_eq!(switch_count, 2, "FPI variant results should validate the tag before storing");
        assert_eq!(
            unreachable_count, 2,
            "invalid FPI variant result tag should be unreachable before storing"
        );
    }

    #[test]
    fn fpi_lowering_rejects_unsupported_result_type_before_flattening() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        // `u256` has no canonical flattening; the typed FPI validation must reject it before
        // flattening is attempted.
        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([]),
            vec![Type::U256],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::Felt); FPI_ABI_PREFIX_ARGS],
            results: vec![],
            cc: CallConv::Wasm,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-get-u256")),
            &import_func_ty,
            core_function_path("fpi-get-u256"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("unsupported FPI result types must fail with the typed diagnostic"),
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains("result 0") && message.contains("typed FPI only supports"),
                    "unexpected error: {message}"
                );
            }
        }
    }

    #[test]
    fn fpi_lowering_rejects_pointer_enum_payload_before_flattening() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let enum_ty = pointer_payload_variant_type();
        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([enum_ty]),
            vec![Type::Felt],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let mut core_params = vec![AbiParam::new(Type::Felt); FPI_ABI_PREFIX_ARGS];
        core_params.extend([AbiParam::new(Type::I32), AbiParam::new(Type::I32)]);
        let core_func_sig = Signature {
            params: core_params,
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::Wasm,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import_paths(test_import_path("fpi-send-pointer-enum")),
            &import_func_ty,
            core_function_path("fpi-send-pointer-enum"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("pointer-like enum payloads must fail with the typed diagnostic"),
            Err(err) => {
                let message = err.to_string();
                assert!(
                    message.contains("parameter 6::items") && message.contains("pointer-like type"),
                    "unexpected error: {message}"
                );
            }
        }
    }

    /// Returns the signatures of the `hir.dyncall` ops in `function`.
    fn dyncall_signatures(function: FunctionRef) -> Vec<Signature> {
        use midenc_hir::{Operation, WalkResult};

        let mut signatures = Vec::new();
        function
            .borrow()
            .as_operation()
            .prewalk(|op: &Operation| {
                if let Some(call) = op.downcast_ref::<midenc_dialect_hir::Dyncall>() {
                    signatures.push(call.get_signature().clone());
                }
                WalkResult::<()>::Continue(())
            })
            .into_result()
            .expect("operation walk should not fail");
        signatures
    }

    /// A dyncall import is reached through its runtime root, so its lowering emits `hir.dyncall`
    /// and declares nothing on the import's component.
    #[test]
    fn dyncall_import_direct_lowering_emits_dyncall_without_declaring_the_import() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let mut ir = FunctionType::new(
            CallConv::Fast,
            vec![word_type(), Type::Felt, Type::U32],
            vec![Type::Felt],
        );
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::I32),
            ],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::ComponentModel,
        };
        let import = component_import_path("dyncall-authority");
        let stub_component = import.path.without_leaf().to_symbol_name().unwrap();

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import,
            &import_func_ty,
            core_function_path("dyncall-authority"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("dyncall import lowering should build");

        let function = lowered.function_ref().expect("expected function lowering");
        let signatures = dyncall_signatures(function);
        assert_eq!(signatures.len(), 1, "expected exactly one hir.dyncall");
        // The root word is consumed as the dyncall operand, not passed as an argument
        let params: Vec<Type> = signatures[0].params.iter().map(|p| p.ty.clone()).collect();
        assert_eq!(params, vec![Type::Felt, Type::I32]);
        let results: Vec<Type> = signatures[0].results.iter().map(|r| r.ty.clone()).collect();
        assert_eq!(results, vec![Type::Felt]);
        assert_eq!(count_ops(function, |op| op.is::<midenc_dialect_hir::Call>()), 0);
        // Nothing is declared for a runtime target: the import's component does not exist
        assert!(world_builder.find_component(stub_component).is_none());
    }

    /// An FPI import reaches its callee by root, so its path may nest in the component's own
    /// namespace: it lowers without the own-namespace error and declares no stub component.
    #[test]
    fn fpi_import_in_the_own_namespace_is_lowered_without_a_stub_component() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let ir = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args([Type::Felt]),
            vec![Type::Felt],
        );
        let import_func_ty = ComponentFunctionType { ir };
        let mut core_params = vec![AbiParam::new(Type::Felt); FPI_ABI_PREFIX_ARGS];
        core_params.push(AbiParam::new(Type::Felt));
        let core_func_sig = Signature {
            params: core_params,
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::Wasm,
        };
        let import = import_at(
            test_import_path("fpi-send"),
            "miden::test::app::fpi::acme::wallet::wallet::send",
        );
        let stub_component = import.path.without_leaf().to_symbol_name().unwrap();

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import,
            &import_func_ty,
            core_function_path("fpi-send"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("an FPI import nested in the own namespace should lower");

        let function = lowered.function_ref().expect("expected function lowering");
        assert_eq!(count_ops(function, |op| op.is::<midenc_dialect_hir::Call>()), 0);
        assert!(world_builder.find_component(stub_component).is_none());
    }

    /// A dyncall import reaches its callee by root, so its path may nest in the component's own
    /// namespace: it lowers without the own-namespace error and declares no stub component.
    #[test]
    fn dyncall_import_in_the_own_namespace_is_lowered_without_a_stub_component() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let mut ir =
            FunctionType::new(CallConv::Fast, vec![word_type(), Type::Felt], vec![Type::Felt]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::Felt); 5],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::ComponentModel,
        };
        let import = import_at(
            test_import_path("dyncall-authority"),
            "miden::test::app::dyncall::authority",
        );
        let stub_component = import.path.without_leaf().to_symbol_name().unwrap();

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            import,
            &import_func_ty,
            core_function_path("dyncall-authority"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("a dyncall import nested in the own namespace should lower");

        let function = lowered.function_ref().expect("expected function lowering");
        assert_eq!(dyncall_signatures(function).len(), 1, "expected exactly one hir.dyncall");
        assert!(world_builder.find_component(stub_component).is_none());
    }

    /// Imports whose stub components nest (`x::y` and `x::y::z`, as for two dependencies whose
    /// namespaces nest) are declared into one world, each in the stub its path's parent names,
    /// and each path resolves to its own declaration.
    #[test]
    fn imports_with_nested_stub_components_are_declared_in_one_world() {
        let (_context, mut world_builder, _module_builder) = world_with_core_module();
        let signature = Signature {
            params: vec![AbiParam::new(Type::Felt)],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::ComponentModel,
        };

        for (cm_name, path) in [("f", "x::y::f"), ("g", "x::y::z::g")] {
            let import = import_at(test_import_path(cm_name), path);
            let function = declare_import_function(&mut world_builder, &import, &signature)
                .unwrap_or_else(|err| panic!("`{path}` should be declared: {err}"));
            let stub = world_builder
                .find_component(import.path.without_leaf().to_symbol_name().unwrap())
                .expect("the stub component is defined");
            assert!(stub.borrow().is_declaration_only());
            let resolved =
                world_builder.world.borrow().resolve(&import.path).expect("the import resolves");
            assert_eq!(
                resolved.borrow().as_symbol_operation().as_operation_ref(),
                function.as_operation_ref(),
                "`{path}` resolves to its own declaration"
            );
        }
    }

    /// An import declaring the leaf `z` in stub `x::y` beside an import from stub `x::y::z` (two
    /// dependencies whose namespaces nest) is reported in either order: the path `x::y::z` cannot
    /// name both the declaration and the component.
    #[test]
    fn an_import_captured_by_a_nested_stub_component_is_reported() {
        let signature = Signature {
            params: vec![AbiParam::new(Type::Felt)],
            results: vec![AbiParam::new(Type::Felt)],
            cc: CallConv::ComponentModel,
        };
        let captured = ("z", "x::y::z");
        let nested = ("g", "x::y::z::g");

        for (first, second) in [(captured, nested), (nested, captured)] {
            let (_context, mut world_builder, _module_builder) = world_with_core_module();
            let import = import_at(test_import_path(first.0), first.1);
            declare_import_function(&mut world_builder, &import, &signature)
                .unwrap_or_else(|err| panic!("`{}` should be declared: {err}", first.1));
            let import = import_at(test_import_path(second.0), second.1);
            let Err(err) = declare_import_function(&mut world_builder, &import, &signature) else {
                panic!("`x::y::z` cannot name both the declaration and the component");
            };
            assert!(
                err.to_string().contains(
                    "component `x::y` declares `z`, which component `x::y::z` would capture"
                ),
                "unexpected diagnostic: {err}"
            );
        }
    }

    /// A dyncall import whose results need the canonical out-pointer keeps that transformation:
    /// the root is still consumed as the dyncall operand and the flat results are stored out.
    #[test]
    fn dyncall_import_transformed_lowering_stores_results_through_out_pointer() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = two_field_record_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![word_type()], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::Felt),
                AbiParam::new(Type::I32),
            ],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("dyncall-pair"),
            &import_func_ty,
            core_function_path("dyncall-pair"),
            SymbolName::intern("stub"),
            core_func_sig,
        )
        .expect("dyncall import lowering should build");

        let function = lowered.function_ref().expect("expected function lowering");
        let signatures = dyncall_signatures(function);
        assert_eq!(signatures.len(), 1, "expected exactly one hir.dyncall");
        assert!(signatures[0].params.is_empty());
        assert_eq!(signatures[0].results.len(), 2);
        // The flat results are stored to the caller-provided out pointer
        assert!(count_ops(function, |op| op.is::<midenc_dialect_hir::Store>()) >= 1);
    }

    /// Builds a dyncall import type and matching core signature with `num_u64` `u64` arguments
    /// after the root word, plus `extra_felts` trailing `felt` arguments, and an optional result.
    ///
    /// A `result` that does not fit a single flat value is returned through the canonical result
    /// pointer, which import flattening appends to the parameters as a core Wasm `i32`.
    fn wide_dyncall_import(
        num_u64: usize,
        extra_felts: usize,
        result: Option<Type>,
    ) -> (ComponentFunctionType, Signature) {
        let mut ir = FunctionType::new(
            CallConv::Fast,
            core::iter::once(word_type())
                .chain((0..num_u64).map(|_| Type::U64))
                .chain((0..extra_felts).map(|_| Type::Felt))
                .collect::<Vec<_>>(),
            result.into_iter().collect::<Vec<_>>(),
        );
        ir.abi = CallConv::ComponentModel;
        let context = Rc::new(Context::default());
        let core_func_sig = expected_core_signature(
            &flatten_function_type(&context, &ir, CanonicalAbiMode::Import).unwrap(),
        );
        (ComponentFunctionType { ir }, core_func_sig)
    }

    #[test]
    fn dyncall_import_actual_payload_boundary_excludes_root_and_result_pointer() {
        for (u64s, extra, result) in
            [(6, 1, None), (8, 0, None), (7, 1, Some(two_field_record_type()))]
        {
            let (_context, mut world_builder, mut module_builder) = world_with_core_module();
            let (import_func_ty, core_func_sig) = wide_dyncall_import(u64s, extra, result);
            let lowered = generate_import_lowering_function(
                &mut world_builder,
                &mut module_builder,
                component_import_path("dyncall-wide"),
                &import_func_ty,
                core_function_path("dyncall-wide"),
                SymbolName::intern("stub"),
                core_func_sig,
            )
            .expect("root is excluded and the result pointer fits the canonical source budget");
            let signatures = dyncall_signatures(lowered.function_ref().unwrap());
            assert_eq!(signatures.len(), 1);
            assert_eq!(signatures[0].params.len(), u64s + extra);
        }
    }

    #[test]
    fn dyncall_import_accepts_full_payload_with_source_result_pointer() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();
        let (import_func_ty, core_func_sig) =
            wide_dyncall_import(8, 0, Some(two_field_record_type()));
        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("dyncall-wide"),
            &import_func_ty,
            core_function_path("dyncall-wide"),
            SymbolName::intern("stub"),
            core_func_sig,
        );
        let lowered = result.expect("source result pointer is separate from sixteen payload lanes");
        let sigs = dyncall_signatures(lowered.function_ref().unwrap());
        assert_eq!(sigs.len(), 1);
        assert_eq!(sigs[0].params.iter().map(|p| p.ty.size_in_felts()).sum::<usize>(), 16);
    }

    #[test]
    fn dyncall_import_rejects_actual_payload_past_sixteen_lanes() {
        for (u64s, extra, expected) in [(8, 1, 17), (9, 0, 18)] {
            let (_context, mut world_builder, mut module_builder) = world_with_core_module();
            let (import_func_ty, core_func_sig) = wide_dyncall_import(u64s, extra, None);
            let result = generate_import_lowering_function(
                &mut world_builder,
                &mut module_builder,
                component_import_path("dyncall-wide"),
                &import_func_ty,
                core_function_path("dyncall-wide"),
                SymbolName::intern("stub"),
                core_func_sig,
            );
            let message = match result {
                Ok(_) => panic!("oversized actual payload accepted"),
                Err(err) => err.to_string(),
            };
            assert!(message.contains(&format!("{expected} argument field elements")), "{message}");
            assert!(message.contains("16-element context argument ABI"), "{message}");
        }
    }

    #[test]
    fn dyncall_import_retains_core_signature_validation() {
        for same_arity in [false, true] {
            let (_context, mut world_builder, mut module_builder) = world_with_core_module();
            let (import_func_ty, mut core_func_sig) = wide_dyncall_import(6, 1, None);
            if same_arity {
                core_func_sig.params[4] = AbiParam::new(Type::I32);
            } else {
                core_func_sig.params = vec![AbiParam::new(Type::I32)];
            }
            let result = generate_import_lowering_function(
                &mut world_builder,
                &mut module_builder,
                component_import_path("dyncall-wide"),
                &import_func_ty,
                core_function_path("dyncall-wide"),
                SymbolName::intern("stub"),
                core_func_sig,
            );
            let message = match result {
                Ok(_) => panic!("incorrect core signature accepted"),
                Err(err) => err.to_string(),
            };
            assert!(message.contains("core Wasm signature mismatch"), "{message}");
        }
    }

    #[test]
    fn rejects_direct_import_lowering_with_mismatched_core_result_signature() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = scalar_u64_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![],
            results: vec![AbiParam::new(Type::I32)],
            cc: CallConv::ComponentModel,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("mismatched_result"),
            &import_func_ty,
            core_function_path("mismatched_result"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("expected mismatched direct import signature to be rejected"),
            Err(err) => {
                assert!(
                    err.to_string().contains("core Wasm signature"),
                    "unexpected diagnostic: {err}"
                );
            }
        }
    }

    #[test]
    fn rejects_transformed_import_lowering_with_mismatched_core_params() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let variant_ty = unit_only_variant_type();
        let result_ty = two_field_record_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![variant_ty], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        // The flattened import parameters are an i32 discriminant plus the i32 result
        // out-pointer, but the core import declares an i64 discriminant.
        let core_func_sig = Signature {
            params: vec![AbiParam::new(Type::I64), AbiParam::new(Type::I32)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("mismatched_params"),
            &import_func_ty,
            core_function_path("mismatched_params"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("expected mismatched transformed import signature to be rejected"),
            Err(err) => {
                assert!(
                    err.to_string().contains("core Wasm signature"),
                    "unexpected diagnostic: {err}"
                );
            }
        }
    }

    #[test]
    fn rejects_direct_import_lowering_with_unsupported_list_param() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let list_ty = Type::List(Arc::new(Type::U8));
        let mut ir = FunctionType::new(CallConv::Fast, vec![list_ty.clone()], vec![]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };

        let core_func_sig = Signature {
            params: vec![
                AbiParam::sret(Type::from(PointerType::new(Type::U8)), &context),
                AbiParam::new(Type::I32),
            ],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        let result = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("list_param"),
            &import_func_ty,
            core_function_path("list_param"),
            SymbolName::intern("stub"),
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("expected direct list import lowering to be rejected"),
            Err(err) => {
                assert!(
                    err.to_string().contains("unsupported canonical ABI"),
                    "unexpected diagnostic: {err}"
                );
            }
        }
    }

    #[test]
    fn import_stub_is_defined_under_the_given_name() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let variant_ty = unit_only_variant_type();
        let result_ty = two_field_record_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![variant_ty], vec![result_ty]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType { ir };
        let core_func_sig = Signature {
            params: vec![AbiParam::zext(Type::I32, &context), AbiParam::new(Type::I32)],
            results: vec![],
            cc: CallConv::ComponentModel,
        };

        // `::miden::test::test::roundtrip`
        let lowered = generate_import_lowering_function(
            &mut world_builder,
            &mut module_builder,
            component_import_path("roundtrip"),
            &import_func_ty,
            core_function_path("miden:test@1.0.0#roundtrip"),
            SymbolName::intern("test_roundtrip"),
            core_func_sig,
        )
        .expect("import lowering should build");
        let function_ref = lowered.function_ref().expect("expected function lowering");
        assert_eq!(function_ref.borrow().name().as_str(), "test_roundtrip");
        assert!(module_builder.get_function("miden:test@1.0.0#roundtrip").is_none());
    }

    #[test]
    fn dyncall_source_shape_counts_root_and_separates_result_pointer() {
        for result_ty in [None, Some(two_field_record_type())] {
            let (context, mut world, mut module) = world_with_core_module();
            let ir = FunctionType::new(
                CallConv::ComponentModel,
                core::iter::once(word_type()).chain(vec![Type::Felt; 16]).collect::<Vec<_>>(),
                result_ty.into_iter().collect::<Vec<_>>(),
            );
            let sig = expected_core_signature(
                &flatten_function_type(&context, &ir, CanonicalAbiMode::Import).unwrap(),
            );
            assert_eq!(sig.params().len(), 1 + usize::from(!ir.results.is_empty()));
            let lowered = generate_import_lowering_function(
                &mut world,
                &mut module,
                component_import_path("dyncall-full-tuple"),
                &ComponentFunctionType { ir },
                core_function_path("dyncall-full-tuple"),
                SymbolName::intern("stub"),
                sig,
            )
            .expect("full canonical tuple is loaded before dyncall");
            let sigs = dyncall_signatures(lowered.function_ref().unwrap());
            assert_eq!(sigs[0].params.len(), 16);
            assert!(sigs[0].params.iter().all(|p| p.ty == Type::Felt));
        }
    }

    #[test]
    fn ordinary_import_target_capacity_is_independent_of_canonical_shape() {
        for (u64s, extra, results, expected) in [
            (8, 0, vec![], None),
            (8, 1, vec![], Some("17 argument field elements")),
            (
                0,
                0,
                vec![Type::from(StructType::new(vec![Type::Felt; 17]))],
                Some("17 result field elements"),
            ),
        ] {
            let (context, mut world, mut module) = world_with_core_module();
            let ir = FunctionType::new(
                CallConv::ComponentModel,
                vec![Type::U64; u64s]
                    .into_iter()
                    .chain(vec![Type::Felt; extra])
                    .collect::<Vec<_>>(),
                results,
            );
            let sig = expected_core_signature(
                &flatten_function_type(&context, &ir, CanonicalAbiMode::Import).unwrap(),
            );
            let outcome = generate_import_lowering_function(
                &mut world,
                &mut module,
                component_import_path("ordinary-capacity"),
                &ComponentFunctionType { ir },
                core_function_path("ordinary-capacity"),
                SymbolName::intern("stub"),
                sig,
            );
            if let Some(expected) = expected {
                let message = outcome.err().expect("actual target overflow must fail").to_string();
                assert!(
                    message.contains(expected)
                        && message.contains("16-element")
                        && message.contains("ordinary-capacity"),
                    "{message}"
                );
            } else {
                outcome.expect("sixteen actual lanes are supported");
            }
        }
    }

    #[test]
    fn dyncall_canonical_tuple_preserves_typed_carriers_and_layout() {
        let (context, mut world, mut module) = world_with_core_module();
        let nested = Type::from(StructType::new(vec![Type::U8, Type::U64, Type::Felt]));
        let variant = mixed_payload_variant_type();
        let ir = FunctionType::new(
            CallConv::ComponentModel,
            core::iter::once(word_type())
                .chain([nested, variant])
                .chain(vec![Type::Felt; 8])
                .collect::<Vec<_>>(),
            vec![],
        );
        let expected = flatten_types(&context, &ir.params[1..]).unwrap();
        let source = flatten_function_type(&context, &ir, CanonicalAbiMode::Import).unwrap();
        assert!(source.params()[0].ty.is_pointer());
        let core_sig = expected_core_signature(&source);
        let lowered = generate_import_lowering_function(
            &mut world,
            &mut module,
            component_import_path("dyncall-typed-tuple"),
            &ComponentFunctionType { ir },
            core_function_path("dyncall-typed-tuple"),
            SymbolName::intern("stub"),
            core_sig,
        )
        .expect("typed tuple arguments normalize before dyncall");
        let actual = dyncall_signatures(lowered.function_ref().unwrap());
        assert_eq!(actual[0].params, expected);
        assert_eq!(actual[0].params[1].ty, Type::I64);
    }

    #[test]
    fn dyncall_canonical_tuple_validates_variants() {
        let (context, mut world, mut module) = world_with_core_module();
        let ir = FunctionType::new(
            CallConv::ComponentModel,
            core::iter::once(word_type())
                .chain([scalar_payload_variant_type()])
                .chain(vec![Type::Felt; 11])
                .collect::<Vec<_>>(),
            vec![],
        );
        let source = flatten_function_type(&context, &ir, CanonicalAbiMode::Import).unwrap();
        assert!(source.params()[0].ty.is_pointer());
        let lowered = generate_import_lowering_function(
            &mut world,
            &mut module,
            component_import_path("dyncall-variant-tuple"),
            &ComponentFunctionType { ir },
            core_function_path("dyncall-variant-tuple"),
            SymbolName::intern("stub"),
            expected_core_signature(&source),
        )
        .expect("tuple variants load with validation");
        let (switches, traps) = count_validation_ops(lowered.function_ref().unwrap());
        assert_eq!((switches, traps), (1, 1));
    }

    #[test]
    fn fpi_genuine_canonical_tuple_preserves_payload_and_result_pointer() {
        let context = Rc::new(Context::default());
        for result in [Type::Felt, two_field_record_type()] {
            let ir = FunctionType::new(
                CallConv::ComponentModel,
                fpi_params_with_user_args(vec![Type::Felt; 11]),
                vec![result],
            );
            let shape = plan_fpi_call_for(&context, &ir, "fpi-genuine-tuple").unwrap();
            assert!(shape.has_arg_ptr);
            assert_eq!(shape.flattened_params.len(), 17);
            assert_eq!(shape.flattened_arg_felts, 17);
            assert_eq!(shape.has_output_ptr, ir.results[0] != Type::Felt);
        }
    }
}

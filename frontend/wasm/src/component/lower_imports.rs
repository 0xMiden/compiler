//! lowering the imports into the Miden ABI for the cross-context calls

use alloc::rc::Rc;
use core::cell::RefCell;

use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::ControlFlowOpBuilder;
use midenc_dialect_hir::{ExecFpi, HirOpBuilder};
use midenc_hir::{
    AddressSpace, Builder, FunctionType, Op, PointerType, SourceSpan, SymbolPath, Type, ValueRef,
    Visibility,
    diagnostics::WrapErr,
    dialects::builtin::{
        BuiltinOpBuilder, ComponentBuilder, ComponentId, ModuleBuilder, WorldBuilder,
        attributes::{AbiParam, Signature},
    },
};
use midenc_session::diagnostics::Report;

use super::{
    ComponentFunctionType, MAX_DIRECT_STACK_FELTS, MAX_FLAT_PARAMS, MAX_FLAT_RESULTS,
    canon_abi_utils::{store, validate_flat_variants},
    flat::{
        CanonicalAbiMode, CanonicalAbiTransformation, check_core_wasm_signature_equivalence,
        classify_function_type, flat_params_need_tuple, flatten_function_type, flatten_types,
        flattened_types_layout,
    },
};
use crate::{
    callable::CallableFunction,
    error::WasmResult,
    fpi::store_fpi_prefix_locals,
    module::function_builder_ext::{
        FunctionBuilderContext, FunctionBuilderExt, SSABuilderListener,
    },
};

const FPI_IMPORT_PREFIX: &str = "fpi-";
const FPI_ABI_PREFIX_ARGS: usize = ExecFpi::PREFIX_FELTS;
const FPI_EXEC_INPUTS: usize = ExecFpi::MAX_INPUT_FELTS;
const FPI_EXEC_RESULTS: usize = ExecFpi::EXECUTOR_RESULT_FELTS;

/// Generates the lowering function (cross-context Miden ABI -> Wasm CABI) for the given import function.
pub fn generate_import_lowering_function(
    world_builder: &mut WorldBuilder,
    module_builder: &mut ModuleBuilder,
    import_func_path: SymbolPath,
    import_func_ty: &ComponentFunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
) -> WasmResult<CallableFunction> {
    let context = module_builder.builder().context_rc();
    // FPI imports bypass canonical ABI validation and classification: they use their own
    // typed-signature checks, and oversized argument lists take the FPI indirect lowering
    // path instead of tupled parameters.
    let is_fpi = is_fpi_import(&import_func_path, &import_func_ty.ir)?;
    if !is_fpi {
        reject_unsupported_import_canonical_abi_types(&import_func_path, import_func_ty)?;
    }
    let import_lowered_sig =
        flatten_function_type(&context, &import_func_ty.ir, CanonicalAbiMode::Import)
            .wrap_err_with(|| {
                format!(
                    "failed to generate component import lowering: signature of \
                     '{import_func_path}' requires flattening"
                )
            })?;
    let transformation = if is_fpi {
        None
    } else {
        let transformation =
            classify_function_type(&context, &import_func_ty.ir).wrap_err_with(|| {
                format!(
                    "failed to generate component import lowering: signature of \
                     '{import_func_path}' requires classification"
                )
            })?;
        // Import flattening appends a result out-pointer after tuple classification, so the
        // final flattened parameter list can exceed the budget even when classification
        // reported no parameter tuple.
        if transformation.has_param_tuple() || flat_params_need_tuple(import_lowered_sig.params()) {
            return reject_tuple_parameter_import_lowering(&import_func_path);
        }
        Some(transformation)
    };

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

    match transformation {
        CanonicalAbiTransformation::None => generate_direct_lowering(
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
        ),
        CanonicalAbiTransformation::ResultOutPtr => generate_lowering_with_transformation(
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
        ),
        CanonicalAbiTransformation::ParamTuple | CanonicalAbiTransformation::Both => {
            unreachable!("tuple-parameter import lowering was rejected earlier")
        }
    }
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
    validate_fpi_typed_signature(&core_func_path, &import_func_ty.ir)?;
    let shape = plan_fpi_call(
        &context,
        &import_func_ty.ir,
        import_lowered_sig,
        &core_func_path,
        &core_func_sig,
        args.len(),
    )?;
    let lowered = lower_fpi_canonical_args(&shape, &import_func_ty.ir, fb, args, span)
        .wrap_err_with(|| format!("failed to lower FPI import arguments for `{core_func_path}`"))?;

    let prefix_locals =
        store_fpi_prefix_locals(fb, &lowered.fpi_args[..FPI_ABI_PREFIX_ARGS], span)?;
    let procedure_inputs = lowered.fpi_args[FPI_ABI_PREFIX_ARGS..].iter().copied();
    let exec = fb.exec_fpi(prefix_locals, procedure_inputs, span)?;
    let results: Vec<ValueRef> = {
        let borrow = exec.borrow();
        borrow.results().iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
    };

    let exit_block = fb.create_block();
    fb.br(exit_block, vec![], span)?;
    fb.seal_block(exit_block);
    fb.switch_to_block(exit_block);
    let results = lower_fpi_result_felts(fb, &shape.flattened_results, &results, span)?;
    if let Some(output_ptr) = lowered.output_ptr {
        if import_func_ty.results.len() != 1 {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import with an output pointer expected one result type, got {}",
                import_func_ty.results.len()
            )));
        }
        let mut results_iter = results.into_iter();
        store(fb, output_ptr, &import_func_ty.results[0], &mut results_iter, span)?;
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
            for field in struct_ty.fields() {
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
            if !enum_ty.is_c_like() {
                return Err(midenc_session::diagnostics::Report::msg(format!(
                    "FPI import `{import_func_path}` {location} contains non-C-like enum \
                     `{enum_ty}`; typed FPI only supports enums without payload values"
                )));
            }
            validate_fpi_value_type(import_func_path, location, enum_ty.discriminant())?;
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
        | Type::Never
        | Type::I128
        | Type::U128
        | Type::U256
        | Type::F64
        | Type::Function(_) => {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import `{import_func_path}` {location} contains unsupported type `{ty}`; \
                 typed FPI only supports felt-width integers, felt, C-like enums, structs, and \
                 arrays"
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
    // Canonical ABI passes more than 16 flattened parameters indirectly through one pointer; the
    // generated wrapper reloads that tuple so every FPI call lowers to the same felt-only form.
    let has_arg_ptr = flattened_params.len() > MAX_FLAT_PARAMS;
    let has_output_ptr = flattened_results.len() > MAX_FLAT_RESULTS;

    if !has_arg_ptr {
        // The generated wrapper receives all its parameters on the operand stack, so the direct
        // call shape is limited by the stack's addressable window, independent of the FPI
        // protocol's own input limit. Check this before comparing against the lowered core
        // signature: over-budget direct shapes are tupled by canonical ABI flattening, which
        // would otherwise surface as a confusing shape mismatch.
        let stack_felts =
            flattened_params.iter().map(|param| param.ty.size_in_felts()).sum::<usize>()
                + usize::from(has_output_ptr);
        if stack_felts > MAX_DIRECT_STACK_FELTS {
            return Err(midenc_session::diagnostics::Report::msg(format!(
                "FPI import `{core_func_path}` lowers to {stack_felts} operand stack felts after \
                 expanding 64-bit values and result pointers, but direct FPI calls support at \
                 most {MAX_DIRECT_STACK_FELTS}"
            )));
        }
    }

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
    import_func_ty: &FunctionType,
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
                "FPI import with more than 16 flattened params did not receive an argument pointer",
            )
        })?;
        lower_fpi_indirect_args(fb, arg_ptr, &import_func_ty.params, span)?
    } else {
        lower_fpi_direct_args(
            fb,
            &shape.flattened_params,
            &args[..shape.flattened_params.len()],
            span,
        )?
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

/// Loads the felt-only protocol argument list from a canonical ABI argument tuple pointer.
///
/// Canonical ABI passes more than 16 flattened parameters indirectly through one pointer. The
/// generated wrapper reloads every flattened value here, so all FPI calls reach the backend in
/// the same direct, felt-only form.
fn lower_fpi_indirect_args(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    arg_ptr: ValueRef,
    params: &[Type],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    let mut fpi_args = Vec::new();
    for entry in flattened_types_layout(params)? {
        let value = load_fpi_tuple_value(fb, arg_ptr, entry.offset, &entry.ty, span)?;
        push_fpi_arg_felts(fb, &entry.ty, value, &mut fpi_args, span)?;
    }
    Ok(fpi_args)
}

/// Loads one canonical ABI tuple value and converts it to its flattened core form.
fn load_fpi_tuple_value(
    fb: &mut FunctionBuilderExt<'_, impl midenc_hir::Builder>,
    arg_ptr: ValueRef,
    byte_offset: u32,
    ty: &Type,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let addr = if byte_offset == 0 {
        arg_ptr
    } else {
        let byte_offset = i32::try_from(byte_offset).map_err(|_| {
            midenc_session::diagnostics::Report::msg(format!(
                "FPI argument layout contains byte offset {byte_offset}, which does not fit in i32"
            ))
        })?;
        let byte_offset = fb.i32(byte_offset, span);
        fb.add_unchecked(arg_ptr, byte_offset, span)?
    };
    let ptr_ty = Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Byte));
    let typed_ptr = fb.inttoptr(addr, ptr_ty, span)?;
    let value = fb.load(typed_ptr, span)?;

    // Narrow integers are stored in their memory width; extend them to the 32-bit form canonical
    // ABI flattening produces, so the felt conversion sees the correct value (signed values must
    // be sign-extended).
    match ty {
        Type::I8 | Type::I16 => Ok(fb.sext(value, Type::I32, span)?),
        Type::I1 | Type::U8 | Type::U16 => Ok(fb.zext(value, Type::U32, span)?),
        _ => Ok(value),
    }
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
        Type::I64 | Type::U64 => {
            let (high, low) = fb.split2(arg, Type::Felt, span)?;
            fpi_args.push(high);
            fpi_args.push(low);
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
        Type::I64 | Type::U64 => {
            let high = take_value(fpi_results, next_result, "FPI result")?;
            let low = take_value(fpi_results, next_result, "FPI result")?;
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
            if is_fpi_felt_type(account_id_prefix)
                && is_fpi_felt_type(account_id_suffix)
                && is_fpi_proc_root_type(proc_root)
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
        .all(is_fpi_felt_type)
    )
}

/// Returns true when `ty` matches the generated FPI `felt` type.
fn is_fpi_felt_type(ty: &Type) -> bool {
    matches!(ty, Type::Felt)
        || matches!(
            ty,
            Type::Struct(struct_ty)
                if struct_ty.fields().len() == 1
                    && struct_ty.fields()[0].offset == 0
                    && struct_ty.fields()[0].ty == Type::Felt
        )
}

/// Returns true when `ty` matches the generated FPI procedure-root `word` record.
fn is_fpi_proc_root_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Struct(struct_ty)
            if struct_ty.fields().len() == 4
                && struct_ty.fields().iter().all(|field| is_fpi_felt_type(&field.ty))
    )
}

/// Rejects component import signatures that require tuple-parameter lowering.
fn reject_tuple_parameter_import_lowering<T>(import_func_path: &SymbolPath) -> WasmResult<T> {
    Err(Report::msg(format!(
        "tuple-parameter import lowering is not supported for '{import_func_path}'"
    )))
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
    import_func_ty: &ComponentFunctionType,
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

    // The lowered core function takes the flattened parameters with the result out-pointer
    // passed as a core Wasm i32 pointer, and returns nothing.
    let mut expected_core_params = import_func_sig_flat.params().to_vec();
    *expected_core_params
        .last_mut()
        .expect("flattened import params cannot be empty") = AbiParam::new(Type::I32);
    let expected_core_sig = Signature {
        params: expected_core_params,
        results: vec![],
        cc: core_func_sig.cc,
    };
    check_core_wasm_signature_equivalence(&core_func_sig, &expected_core_sig).map_err(
        |message| {
            Report::msg(format!(
                "component import lowering for '{import_func_path}' has core Wasm signature \
                 mismatch: {message}"
            ))
        },
    )?;

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

    // Extract the actual result types from the import function type
    let flattened_results =
        flatten_types(&context, &import_func_ty.ir.results).wrap_err_with(|| {
            format!("failed to flatten result types for import function '{import_func_path}'")
        })?;

    // Remove the pointer parameter that was added for the flattened signature
    let params_without_ptr =
        import_func_sig_flat.params[..import_func_sig_flat.params.len() - 1].to_vec();
    let new_import_func_sig = Signature {
        params: params_without_ptr,
        results: flattened_results,
        cc: import_func_sig_flat.cc,
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

    validate_flat_variants(fb, &import_func_ty.params, &args_without_ptr, span)?;

    // Call the import function - it will return a tuple to the flattened result
    let call = fb.call(import_func_ref, new_import_func_sig, args_without_ptr, span)?;

    let borrow = call.borrow();
    let results_storage = borrow.results();
    let results: Vec<ValueRef> =
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect();
    validate_flat_variants(fb, &import_func_ty.results, &results, span)?;

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
/// * `import_func_sig_flat` - The flattened signature of the component import.
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
    import_func_ty: &ComponentFunctionType,
    core_func_path: SymbolPath,
    core_func_sig: Signature,
    import_func_sig_flat: Signature,
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

    validate_flat_variants(fb, &import_func_ty.params, args, span)?;

    check_core_wasm_signature_equivalence(&core_func_sig, &import_func_sig_flat).map_err(
        |message| {
            Report::msg(format!(
                "component import lowering for '{import_func_path}' has core Wasm signature \
                 mismatch: {message}"
            ))
        },
    )?;
    let import_func_ref = component_builder
        .define_function(
            import_func_path.name().into(),
            Visibility::Internal,
            import_func_sig_flat.clone(),
        )
        .expect("failed to define the import function");

    let call = fb
        .call(import_func_ref, import_func_sig_flat, args.to_vec(), span)
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
    validate_flat_variants(fb, &import_func_ty.results, &results, span)?;

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

/// Rejects component import signatures containing unsupported canonical ABI shapes.
fn reject_unsupported_import_canonical_abi_types(
    import_func_path: &SymbolPath,
    import_func_ty: &ComponentFunctionType,
) -> WasmResult<()> {
    for ty in import_func_ty.params.iter().chain(import_func_ty.results.iter()) {
        if ty.contains_unsupported() {
            return Err(Report::msg(format!(
                "component import lowering for '{import_func_path}' has unsupported canonical ABI \
                 type {:?}",
                ty.ir
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use alloc::sync::Arc;

    use midenc_hir::{
        CallConv, Context, EnumType, FunctionType, PointerType, StructType, SymbolName,
        SymbolNameComponent, SymbolPath, Type, Variant, dialects::builtin::attributes::AbiParam,
        interner::Symbol,
    };

    use super::*;
    use crate::component::{
        CanonicalAbiInfo, CanonicalAbiType, CanonicalAbiTypeKind,
        test_support::{
            count_validation_ops, scalar_payload_variant_type, two_field_record_type,
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
            Arc::from("miden:base/core-types@1.0.0/felt"),
            [(Arc::from("inner"), Type::Felt)],
        ))
    }

    fn wit_word_type() -> Type {
        let felt_ty = wit_felt_type();
        Type::from(StructType::named(
            Arc::from("miden:base/core-types@1.0.0/word"),
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
    fn validate_fpi_typed_signature_rejects_non_c_like_enum_param() {
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "result".into(),
                Type::U8,
                [
                    Variant::c_like("ok".into(), Some(0)),
                    Variant::new("err".into(), Type::I32, Some(1)),
                ],
            )
            .unwrap(),
        ));
        let import_func_ty = FunctionType::new(
            CallConv::Wasm,
            fpi_params_with_user_args([enum_ty]),
            vec![Type::Felt],
        );
        let import_func_path = test_import_path("fpi-read-non-c-like-enum");

        let err = validate_fpi_typed_signature(&import_func_path, &import_func_ty)
            .expect_err("non-C-like enum parameters must be rejected before flattening");
        let message = err.to_string();

        assert!(
            message.contains("parameter 6")
                && message.contains("non-C-like enum")
                && message.contains("enums without payload values"),
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
        plan_fpi_call(
            context,
            import_func_ty,
            &import_lowered_sig,
            &test_import_path(core_func_name),
            &import_lowered_sig,
            import_lowered_sig.params().len(),
        )
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
    fn plan_fpi_call_rejects_direct_calls_past_the_stack_window() {
        let context = Rc::new(Context::default());
        // Six `u64` values plus one felt fit in 13 canonical flat parameters (direct shape), but
        // expand to 19 operand stack felts on the wrapper call.
        let import_func_ty = FunctionType::new(
            CallConv::ComponentModel,
            fpi_params_with_user_args((0..6).map(|_| Type::U64).chain([Type::Felt])),
            vec![Type::Felt],
        );

        let err = plan_fpi_call_for(&context, &import_func_ty, "fpi-echo-six-u64-record")
            .expect_err("expected FPI validation to reject wide direct wrapper calls");
        let message = err.to_string();

        assert!(
            message.contains("lowers to 19 operand stack felts")
                && message.contains("direct FPI calls support at most 16"),
            "unexpected error message: {message}"
        );
    }

    #[test]
    fn flattened_types_layout_rejects_non_c_like_enum() {
        let enum_ty = Type::Enum(Arc::new(
            EnumType::new(
                "result".into(),
                Type::U8,
                [
                    Variant::c_like("ok".into(), Some(0)),
                    Variant::new("err".into(), Type::I32, Some(1)),
                ],
            )
            .unwrap(),
        ));

        let err = match flattened_types_layout(&[enum_ty]) {
            Ok(_) => panic!("non-C-like enum layouts must return a diagnostic"),
            Err(err) => err,
        };
        let message = err.to_string();

        assert!(message.contains("non-C-like enum"), "unexpected error: {message}");
    }

    fn component_import_path(function: &str) -> SymbolPath {
        SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("miden:test@1.0.0")),
            SymbolNameComponent::Leaf(SymbolName::intern(function)),
        ])
    }

    fn core_function_path(function: &str) -> SymbolPath {
        SymbolPath::from_iter([
            SymbolNameComponent::Root,
            SymbolNameComponent::Component(SymbolName::intern("core")),
            SymbolNameComponent::Leaf(SymbolName::intern(function)),
        ])
    }

    fn scalar_i32_type() -> CanonicalAbiType {
        CanonicalAbiType {
            ir: Type::I32,
            abi: CanonicalAbiInfo::SCALAR4,
            kind: CanonicalAbiTypeKind::Scalar,
        }
    }

    fn scalar_u64_type() -> CanonicalAbiType {
        CanonicalAbiType {
            ir: Type::U64,
            abi: CanonicalAbiInfo::SCALAR8,
            kind: CanonicalAbiTypeKind::Scalar,
        }
    }

    #[test]
    fn rejects_import_lowering_with_tupled_params_and_no_result() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let mut ir = FunctionType::new(CallConv::Fast, vec![Type::I32; 17], vec![]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([]),
            results: Box::new([]),
        };

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
    fn rejects_import_lowering_when_result_out_pointer_exceeds_param_budget() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = two_field_record_type();
        let mut ir =
            FunctionType::new(CallConv::Fast, vec![Type::I32; 16], vec![result_ty.ir.clone()]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: (0..16).map(|_| scalar_i32_type()).collect::<Vec<_>>().into_boxed_slice(),
            results: Box::new([result_ty]),
        };

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
            core_func_sig,
        );

        match result {
            Ok(_) => panic!("expected import out-pointer overflow to be rejected"),
            Err(err) => {
                assert!(
                    err.to_string().contains("tuple-parameter import lowering"),
                    "unexpected diagnostic: {err}"
                );
            }
        }
    }

    #[test]
    fn transformed_import_lowering_validates_flat_variant_params() {
        let (context, mut world_builder, mut module_builder) = world_with_core_module();

        let variant_ty = unit_only_variant_type();
        let result_ty = two_field_record_type();
        let mut ir = FunctionType::new(
            CallConv::Fast,
            vec![variant_ty.ir.clone()],
            vec![result_ty.ir.clone()],
        );
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([variant_ty]),
            results: Box::new([result_ty.clone()]),
        };
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
        let mut ir = FunctionType::new(CallConv::Fast, vec![], vec![result_ty.ir.clone()]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([]),
            results: Box::new([result_ty]),
        };
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
    fn rejects_direct_import_lowering_with_mismatched_core_result_signature() {
        let (_context, mut world_builder, mut module_builder) = world_with_core_module();

        let result_ty = scalar_u64_type();
        let mut ir = FunctionType::new(CallConv::Fast, vec![], vec![result_ty.ir.clone()]);
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([]),
            results: Box::new([result_ty]),
        };
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
        let mut ir = FunctionType::new(
            CallConv::Fast,
            vec![variant_ty.ir.clone()],
            vec![result_ty.ir.clone()],
        );
        ir.abi = CallConv::ComponentModel;
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([variant_ty]),
            results: Box::new([result_ty]),
        };
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
        let import_func_ty = ComponentFunctionType {
            ir,
            params: Box::new([CanonicalAbiType {
                ir: list_ty,
                abi: CanonicalAbiInfo::POINTER_PAIR,
                kind: CanonicalAbiTypeKind::Unsupported,
            }]),
            results: Box::new([]),
        };

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
}

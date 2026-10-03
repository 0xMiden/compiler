use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::{ExecFpi, HirOpBuilder};
use midenc_hir::{
    AddressSpace, Builder, Immediate, Op, PointerType, SourceSpan, Type, ValueRef,
    dialects::builtin::FunctionRef,
};
use midenc_package_interface::{LoweredSignature, ReturnArea, lower_signature};
use midenc_session::diagnostics::Report;

use super::resolve::convert_results;
use crate::{
    error::WasmResult, fpi::store_fpi_prefix_locals,
    module::function_builder_ext::FunctionBuilderExt,
};

const RAW_FPI_FLATTENED_ARG_COUNT: u32 = ExecFpi::EXECUTOR_INPUT_FELTS as u32;
const RAW_FPI_FLATTENED_ARG_COUNT_USIZE: usize = ExecFpi::EXECUTOR_INPUT_FELTS;

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

/// The Miden ABI function returns felts on the stack and we want to return via a pointer argument
///
/// `lowered` is the shape the Miden ABI rule set derives for the callee: its results are converted
/// back to their Wasm carrier types before they are stored, so the return area keeps the layout
/// the stub's Wasm signature implies (see [`super::resolve::convert_results`]).
pub fn return_via_pointer<B: ?Sized + Builder>(
    import_func_ref: FunctionRef,
    lowered: &LoweredSignature,
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

    let results: Vec<ValueRef> = {
        let borrow = exec.borrow();
        let results_storage = borrow.results();
        results_storage.iter().map(|op_res| op_res.borrow().as_value_ref()).collect()
    };
    let results = convert_results(lowered, &results, builder, span)?;

    let area = lowered.return_area().expect("return_via_pointer is only used for OutPointer");
    store_results_to_pointer(&area, &results, *ptr_arg, builder)?;

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

    let area = lower_signature(&crate::intrinsics::fpi::signature())
        .expect("the FPI signature lowers")
        .return_area()
        .expect("sixteen felts go through an out pointer");
    store_results_to_pointer(&area, &results, *ptr_arg, builder)?;

    Ok(Vec::new())
}

/// Store `results` through `ptr_arg` at the offsets `area` gives them.
///
/// `area` is [`LoweredSignature::return_area`]: the same layout a generated wrapper reads back,
/// so the two consumers of the rule set cannot disagree about where a result lives.
pub(crate) fn store_results_to_pointer<B: ?Sized + Builder>(
    area: &ReturnArea,
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
    if area.slots.len() != results.len() {
        return Err(Report::msg(format!(
            "return area has {} slots but the callee produced {} results",
            area.slots.len(),
            results.len()
        )));
    }

    let ptr_u32 = builder.bitcast(ptr_arg, Type::U32, span)?;

    for (slot, value) in area.slots.iter().zip(results) {
        let value_ty = (*value).borrow().ty().clone();
        // The slot is sized for the carrier, so any other type would write the wrong width.
        let carrier = slot.scalar.frontend_type();
        if value_ty != carrier {
            return Err(Report::msg(format!(
                "return area slot at offset {} holds a `{carrier}`, but the value stored there is \
                 a `{value_ty}`",
                slot.offset
            )));
        }
        let eff_ptr = if slot.offset == 0 {
            // We're assuming here that the base pointer is of the correct alignment
            ptr_u32
        } else {
            let imm_val = builder.imm(Immediate::U32(slot.offset), span);
            builder.add(ptr_u32, imm_val, span)?
        };
        let addr = builder.inttoptr(eff_ptr, Type::from(PointerType::new(value_ty)), span)?;
        builder.store(addr, *value, span)?;
    }

    Ok(())
}

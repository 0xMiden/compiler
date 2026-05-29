use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_cf::{ControlFlowOpBuilder, SwitchCase};
use midenc_dialect_hir::HirOpBuilder;
use midenc_dialect_ub::UndefinedBehaviorOpBuilder;
use midenc_hir::{
    AddressSpace, BlockRef, Builder, Felt, PointerType, SmallVec, SourceSpan, Type, ValueRef,
};

use super::{CanonicalAbiType, CanonicalAbiTypeKind};
use crate::{WasmError, error::WasmResult, module::function_builder_ext::FunctionBuilderExt};

/// Recursively loads primitive values from memory based on the component-level type following the
/// canonical ABI loading algorithm from
/// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#loading
pub fn load<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    ty: &CanonicalAbiType,
    values: &mut SmallVec<[ValueRef; 8]>,
    span: SourceSpan,
) -> WasmResult<()> {
    match &ty.kind {
        // Primitive types are loaded directly
        CanonicalAbiTypeKind::Scalar => {
            values.push(load_scalar_value(fb, ptr, &ty.ir, span)?);
        }

        CanonicalAbiTypeKind::Variant {
            discriminant,
            payload_offset32,
            cases,
            payload_flat_types,
        } => {
            load(fb, ptr, discriminant, values, span)?;
            let discriminant = *values.last().expect("variant load should produce a discriminant");
            load_variant_payload(
                fb,
                ptr,
                discriminant,
                *payload_offset32,
                cases,
                payload_flat_types,
                values,
                span,
            )?;
        }

        // Struct types are loaded field by field
        CanonicalAbiTypeKind::Record { fields } => {
            for field in fields {
                let field_addr = offset_addr(fb, ptr, field.offset32, span)?;
                // Recursively load the field
                load(fb, field_addr, &field.ty, values, span)?;
            }
        }

        CanonicalAbiTypeKind::Unsupported if matches!(&ty.ir, Type::List(_)) => {
            unimplemented!("List types are not yet supported in cross-context calls")
        }

        CanonicalAbiTypeKind::Unsupported => {
            return Err(WasmError::Unsupported(format!(
                "Unsupported type in canonical ABI loading: {:?}",
                ty.ir
            ))
            .into());
        }
    }

    Ok(())
}

/// Recursively stores primitive values to memory based on the component-level type following the
/// canonical ABI storing algorithm from
/// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#storing
pub fn store<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    ty: &CanonicalAbiType,
    values: &mut impl Iterator<Item = ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
    match &ty.kind {
        // Primitive types are stored directly
        CanonicalAbiTypeKind::Scalar => {
            let value_to_store = values.next().expect("Not enough values to store");
            store_scalar_value(fb, ptr, &ty.ir, value_to_store, span)?;
        }

        CanonicalAbiTypeKind::Variant {
            discriminant,
            payload_offset32,
            cases,
            payload_flat_types,
        } => {
            let discriminant_value = values.next().expect("Not enough values to store");
            store_scalar_value(fb, ptr, &discriminant.ir, discriminant_value, span)?;
            store_variant_payload(
                fb,
                ptr,
                discriminant_value,
                *payload_offset32,
                cases,
                payload_flat_types,
                values,
                span,
            )?;
        }

        // Struct types are stored field by field
        CanonicalAbiTypeKind::Record { fields } => {
            for field in fields {
                let field_addr = offset_addr(fb, ptr, field.offset32, span)?;
                // Recursively store the field
                store(fb, field_addr, &field.ty, values, span)?;
            }
        }

        CanonicalAbiTypeKind::Unsupported if matches!(&ty.ir, Type::List(_)) => {
            unimplemented!("List types are not yet supported in cross-context calls")
        }

        CanonicalAbiTypeKind::Unsupported => {
            return Err(WasmError::Unsupported(format!(
                "Unsupported type in canonical ABI storing: {:?}",
                ty.ir
            ))
            .into());
        }
    }

    Ok(())
}

/// Loads the active case payload of a canonical ABI variant.
#[allow(clippy::too_many_arguments)]
fn load_variant_payload<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    discriminant: ValueRef,
    payload_offset32: u32,
    cases: &[Option<CanonicalAbiType>],
    payload_flat_types: &[Type],
    values: &mut SmallVec<[ValueRef; 8]>,
    span: SourceSpan,
) -> WasmResult<()> {
    if payload_flat_types.is_empty() {
        return Ok(());
    }

    let payload_addr = offset_addr(fb, ptr, payload_offset32, span)?;
    let join_block = fb.create_block_with_params(payload_flat_types.iter().cloned(), span);
    let case_blocks = (0..cases.len()).map(|_| fb.create_block()).collect::<Vec<_>>();
    let default_block = fb.create_block();
    let selector = switch_selector(fb, discriminant, span)?;
    let switch_cases = case_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| SwitchCase::create(index as u32, *block, Vec::new()));
    fb.switch(selector, switch_cases, default_block, [], span)?;

    for block in &case_blocks {
        fb.seal_block(*block);
    }
    fb.seal_block(default_block);

    fb.switch_to_block(default_block);
    fb.unreachable(span);

    for (block, case) in case_blocks.into_iter().zip(cases) {
        fb.switch_to_block(block);
        let case_values = load_case_payload(fb, payload_addr, case.as_ref(), span)?;
        let joined_values = adapt_flat_values(fb, &case_values, payload_flat_types, span)?;
        fb.br(join_block, joined_values, span)?;
    }

    fb.seal_block(join_block);
    fb.switch_to_block(join_block);
    values.extend(block_arguments(join_block));
    Ok(())
}

/// Stores the active case payload of a canonical ABI variant.
#[allow(clippy::too_many_arguments)]
fn store_variant_payload<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    discriminant: ValueRef,
    payload_offset32: u32,
    cases: &[Option<CanonicalAbiType>],
    payload_flat_types: &[Type],
    values: &mut impl Iterator<Item = ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
    if payload_flat_types.is_empty() {
        return Ok(());
    }

    let payload_values = payload_flat_types
        .iter()
        .map(|_| values.next().expect("Not enough values to store"))
        .collect::<Vec<_>>();
    let payload_addr = offset_addr(fb, ptr, payload_offset32, span)?;
    let join_block = fb.create_block();
    let case_blocks = (0..cases.len()).map(|_| fb.create_block()).collect::<Vec<_>>();
    let default_block = fb.create_block();
    let selector = switch_selector(fb, discriminant, span)?;
    let switch_cases = case_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| SwitchCase::create(index as u32, *block, Vec::new()));
    fb.switch(selector, switch_cases, default_block, [], span)?;

    for block in &case_blocks {
        fb.seal_block(*block);
    }
    fb.seal_block(default_block);

    fb.switch_to_block(default_block);
    fb.unreachable(span);

    for (block, case) in case_blocks.into_iter().zip(cases) {
        fb.switch_to_block(block);
        if let Some(case_ty) = case {
            let case_flat_types = case_ty.flat_types();
            let case_values = project_flat_values(fb, &payload_values, &case_flat_types, span)?;
            let mut case_values = case_values.into_iter();
            store(fb, payload_addr, case_ty, &mut case_values, span)?;
        }
        fb.br(join_block, [], span)?;
    }

    fb.seal_block(join_block);
    fb.switch_to_block(join_block);
    Ok(())
}

/// Loads the flattened payload values for one variant case.
fn load_case_payload<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    payload_addr: ValueRef,
    case: Option<&CanonicalAbiType>,
    span: SourceSpan,
) -> WasmResult<SmallVec<[ValueRef; 8]>> {
    let mut values = SmallVec::<[ValueRef; 8]>::new();
    if let Some(case_ty) = case {
        load(fb, payload_addr, case_ty, &mut values, span)?;
    }
    Ok(values)
}

/// Adapts one case's flat payload values to the joined variant payload slots.
fn adapt_flat_values<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    values: &[ValueRef],
    target_types: &[Type],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    assert!(
        values.len() <= target_types.len(),
        "variant case produced more flat payload values than the joined payload shape"
    );

    let mut adapted = Vec::with_capacity(target_types.len());
    for (value, target_ty) in values.iter().zip(target_types) {
        adapted.push(convert_flat_value(fb, *value, target_ty, span)?);
    }
    for target_ty in target_types.iter().skip(values.len()) {
        adapted.push(zero_flat_value(fb, target_ty, span));
    }
    Ok(adapted)
}

/// Projects joined variant payload slots into the active case's flat payload shape.
fn project_flat_values<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    values: &[ValueRef],
    target_types: &[Type],
    span: SourceSpan,
) -> WasmResult<Vec<ValueRef>> {
    assert!(
        target_types.len() <= values.len(),
        "variant case requires more flat payload values than the joined payload shape"
    );

    values
        .iter()
        .zip(target_types)
        .map(|(value, target_ty)| convert_flat_value(fb, *value, target_ty, span))
        .collect()
}

/// Loads a scalar memory value and converts it to its flattened CanonABI value type.
fn load_scalar_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    ty: &Type,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let ptr_type = Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Byte));
    let typed_ptr = fb.inttoptr(ptr, ptr_type, span)?;
    let value = fb.load(typed_ptr, span)?;
    widen_loaded_value(fb, value, ty, span)
}

/// Stores one flattened CanonABI scalar value to memory.
fn store_scalar_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    ty: &Type,
    value: ValueRef,
    span: SourceSpan,
) -> WasmResult<()> {
    let ptr_type = Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Byte));
    let src_ptr = fb.inttoptr(ptr, ptr_type, span)?;
    let value = narrow_stored_value(fb, value, ty, span)?;
    fb.store(src_ptr, value, span)?;
    Ok(())
}

/// Converts a loaded memory value into its flattened CanonABI value type.
fn widen_loaded_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    value: ValueRef,
    ty: &Type,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    Ok(match ty {
        Type::I1 | Type::U8 | Type::U16 => {
            let value = fb.zext(value, Type::U32, span)?;
            fb.bitcast(value, Type::I32, span)?
        }
        Type::I8 | Type::I16 => fb.sext(value, Type::I32, span)?,
        Type::U32 => fb.bitcast(value, Type::I32, span)?,
        Type::U64 => fb.bitcast(value, Type::I64, span)?,
        _ => value,
    })
}

/// Converts a flattened CanonABI value into the type stored in memory.
fn narrow_stored_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    value: ValueRef,
    ty: &Type,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    if value.borrow().ty() == ty {
        return Ok(value);
    }

    Ok(match ty {
        Type::I1 | Type::I8 | Type::U8 | Type::I16 | Type::U16 => {
            fb.trunc(value, ty.clone(), span)?
        }
        Type::U32 | Type::U64 => fb.bitcast(value, ty.clone(), span)?,
        _ => value,
    })
}

/// Converts a flat payload value into the joined or active-case flat type.
fn convert_flat_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    value: ValueRef,
    target_ty: &Type,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let source_ty = value.borrow().ty().clone();
    if &source_ty == target_ty {
        return Ok(value);
    }

    Ok(match target_ty {
        Type::I32 => match source_ty {
            Type::I64 | Type::U64 => fb.trunc(value, Type::I32, span)?,
            Type::U32 | Type::Felt => fb.bitcast(value, Type::I32, span)?,
            _ => fb.cast(value, Type::I32, span)?,
        },
        Type::I64 => match source_ty {
            Type::U64 => fb.bitcast(value, Type::I64, span)?,
            Type::Felt | Type::I32 | Type::U32 => fb.cast(value, Type::I64, span)?,
            _ => fb.cast(value, Type::I64, span)?,
        },
        Type::Felt => match source_ty {
            Type::I32 | Type::U32 => fb.bitcast(value, Type::Felt, span)?,
            Type::I64 | Type::U64 => fb.trunc(value, Type::Felt, span)?,
            _ => fb.cast(value, Type::Felt, span)?,
        },
        Type::U64 => match source_ty {
            Type::I64 => fb.bitcast(value, Type::U64, span)?,
            _ => fb.cast(value, Type::U64, span)?,
        },
        Type::U32 => match source_ty {
            Type::I32 => fb.bitcast(value, Type::U32, span)?,
            Type::I64 | Type::U64 => fb.trunc(value, Type::U32, span)?,
            _ => fb.cast(value, Type::U32, span)?,
        },
        _ => fb.cast(value, target_ty.clone(), span)?,
    })
}

/// Returns a zero value for one canonical flat payload slot.
fn zero_flat_value<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ty: &Type,
    span: SourceSpan,
) -> ValueRef {
    match ty {
        Type::I32 => fb.i32(0, span),
        Type::U32 => fb.u32(0, span),
        Type::I64 => fb.i64(0, span),
        Type::U64 => fb.u64(0, span),
        Type::Felt => fb.felt(Felt::ZERO, span),
        Type::I1 => fb.i1(false, span),
        Type::I8 => fb.i8(0, span),
        Type::U8 => fb.u8(0, span),
        Type::I16 => fb.i16(0, span),
        Type::U16 => fb.u16(0, span),
        ty => unimplemented!("zero value for canonical flat type {ty}"),
    }
}

/// Converts a flat discriminant value into the selector type required by `cf.switch`.
fn switch_selector<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    value: ValueRef,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    let ty = value.borrow().ty().clone();
    Ok(match ty {
        Type::U32 => value,
        Type::I32 => fb.bitcast(value, Type::U32, span)?,
        Type::U8 | Type::U16 => fb.zext(value, Type::U32, span)?,
        Type::I8 | Type::I16 => {
            let value = fb.sext(value, Type::I32, span)?;
            fb.bitcast(value, Type::U32, span)?
        }
        ty => {
            return Err(WasmError::Unsupported(format!(
                "Unsupported canonical ABI variant discriminant type: {ty:?}"
            ))
            .into());
        }
    })
}

/// Returns the value arguments of a block.
fn block_arguments(block: BlockRef) -> Vec<ValueRef> {
    block.borrow().arguments().iter().copied().map(|arg| arg as ValueRef).collect()
}

/// Returns `ptr + offset`, preserving `ptr` when no offset is needed.
fn offset_addr<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    offset: u32,
    span: SourceSpan,
) -> WasmResult<ValueRef> {
    if offset == 0 {
        return Ok(ptr);
    }

    let offset = fb.i32(offset as i32, span);
    fb.add_unchecked(ptr, offset, span)
}

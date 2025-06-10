use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{AddressSpace, Builder, PointerType, SmallVec, SourceSpan, Type, ValueRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt, WasmError};

/// Recursively loads primitive values from memory based on the component-level type following the
/// canonical ABI loading algorithm from
/// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#loading
pub fn load<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    base_ptr: ValueRef,
    ty: &Type,
    offset: &mut u32,
    values: &mut SmallVec<[ValueRef; 8]>,
    span: SourceSpan,
) -> WasmResult<()> {
    // Align offset to the type's alignment requirement
    let alignment = ty.min_alignment() as u32;
    *offset = align_to(*offset, alignment);

    match ty {
        // Primitive types are loaded directly
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::I64
        | Type::U64
        | Type::Felt => {
            let ptr_type =
                Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Byte));

            let src_offset = fb.i32(*offset as i32, span);
            let src_addr = fb.add_unchecked(base_ptr, src_offset, span)?;
            let src_ptr = fb.inttoptr(src_addr, ptr_type, span)?;
            let value = fb.load(src_ptr, span)?;
            values.push(value);

            *offset += ty.size_in_bytes() as u32;
        }

        // Struct types are loaded field by field
        Type::Struct(struct_ty) => {
            // For each field in the struct, use the pre-calculated field offset
            let struct_base_offset = *offset;
            for field in struct_ty.fields() {
                // Use the field's offset within the struct, plus the struct's base offset
                let mut field_offset = struct_base_offset + field.offset;

                // Recursively load the field
                load(fb, base_ptr, &field.ty, &mut field_offset, values, span)?;
            }
            // Update the offset to after the struct
            *offset = struct_base_offset + struct_ty.size() as u32;
        }

        // List types would load pointer and length, but are not supported in the current context
        Type::List(_) => {
            unimplemented!("List types are not yet supported in cross-context calls")
        }

        _ => {
            return Err(WasmError::Unsupported(format!(
                "Unsupported type in canonical ABI loading: {:?}",
                ty
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
    base_ptr: ValueRef,
    ty: &Type,
    offset: &mut u32,
    values: &mut impl Iterator<Item = ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
    // Align offset to the type's alignment requirement
    let alignment = ty.min_alignment() as u32;
    *offset = align_to(*offset, alignment);

    match ty {
        // Primitive types are stored directly
        Type::I1
        | Type::I8
        | Type::U8
        | Type::I16
        | Type::U16
        | Type::I32
        | Type::U32
        | Type::I64
        | Type::U64
        | Type::Felt => {
            let ptr_type =
                Type::from(PointerType::new_with_address_space(ty.clone(), AddressSpace::Byte));

            let mut value = values.next().expect("Not enough values to store");

            // If the value type doesn't match the expected type, cast it
            // This is needed because values returned from component calls may be
            // promoted to larger types (e.g., u8/u16 returned as i32)
            let value_ty = value.borrow().ty().clone();
            if &value_ty != ty {
                match ty {
                    Type::U8 | Type::I8 => {
                        // Cast to u8/i8 by truncating
                        value = fb.cast(value, ty.clone(), span)?;
                    }
                    Type::U16 | Type::I16 => {
                        // Cast to u16/i16 by truncating
                        value = fb.cast(value, ty.clone(), span)?;
                    }
                    _ => {
                        // For other types, assume the value is already the correct type
                    }
                }
            }

            let dst_offset = fb.i32(*offset as i32, span);
            let dst_addr = fb.add_unchecked(base_ptr, dst_offset, span)?;
            let dst_ptr = fb.inttoptr(dst_addr, ptr_type, span)?;
            fb.store(dst_ptr, value, span)?;

            *offset += ty.size_in_bytes() as u32;
        }

        // Struct types are stored field by field
        Type::Struct(struct_ty) => {
            // For each field in the struct, use the pre-calculated field offset
            let struct_base_offset = *offset;
            for field in struct_ty.fields() {
                // Use the field's offset within the struct, plus the struct's base offset
                let mut field_offset = struct_base_offset + field.offset;

                // Recursively store the field
                store(fb, base_ptr, &field.ty, &mut field_offset, values, span)?;
            }
            // Update the offset to after the struct
            *offset = struct_base_offset + struct_ty.size() as u32;
        }

        // List types would store pointer and length, but are not supported in the current context
        Type::List(_) => {
            unimplemented!("List types are not yet supported in cross-context calls")
        }

        _ => {
            return Err(WasmError::Unsupported(format!(
                "Unsupported type in canonical ABI storing: {:?}",
                ty
            ))
            .into());
        }
    }

    Ok(())
}

/// Aligns ("rounds down") a value to the specified alignment (must be a power of 2)
/// from https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#alignment
pub fn align_to(ptr: u32, alignment: u32) -> u32 {
    debug_assert!(alignment.is_power_of_two());
    (ptr / alignment) * alignment
}

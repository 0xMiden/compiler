use midenc_dialect_arith::ArithOpBuilder;
use midenc_dialect_hir::HirOpBuilder;
use midenc_hir::{AddressSpace, Builder, PointerType, SmallVec, SourceSpan, Type, ValueRef};

use crate::{error::WasmResult, module::function_builder_ext::FunctionBuilderExt, WasmError};

/// Recursively loads primitive values from memory based on the component-level type following the
/// canonical ABI loading algorithm from
/// https://github.com/WebAssembly/component-model/blob/main/design/mvp/CanonicalABI.md#loading
pub fn load<B: ?Sized + Builder>(
    fb: &mut FunctionBuilderExt<B>,
    ptr: ValueRef,
    ty: &Type,
    values: &mut SmallVec<[ValueRef; 8]>,
    span: SourceSpan,
) -> WasmResult<()> {
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
            let typed_ptr = fb.inttoptr(ptr, ptr_type, span)?;
            let value = fb.load(typed_ptr, span)?;
            values.push(value);
        }

        // Struct types are loaded field by field
        Type::Struct(struct_ty) => {
            // For each field in the struct, use the pre-calculated field offset
            for field in struct_ty.fields() {
                let field_offset = fb.i32(field.offset as i32, span);
                let fielt_addr = fb.add_unchecked(ptr, field_offset, span)?;
                // Recursively load the field
                load(fb, fielt_addr, &field.ty, values, span)?;
            }
        }

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
    ptr: ValueRef,
    ty: &Type,
    values: &mut impl Iterator<Item = ValueRef>,
    span: SourceSpan,
) -> WasmResult<()> {
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
            let src_ptr = fb.inttoptr(ptr, ptr_type, span)?;
            let value = values.next().expect("Not enough values to store");
            fb.store(src_ptr, value, span)?;
        }

        // Struct types are stored field by field
        Type::Struct(struct_ty) => {
            // For each field in the struct, use the pre-calculated field offset
            for field in struct_ty.fields() {
                let field_offset = fb.i32(field.offset as i32, span);
                let field_addr = fb.add_unchecked(ptr, field_offset, span)?;
                // Recursively store the field
                store(fb, field_addr, &field.ty, values, span)?;
            }
        }

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

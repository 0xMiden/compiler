use alloc::format;

use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::builtin::attributes::U32ArrayAttr,
    effects::*,
    traits::*,
    *,
};

use crate::HirDialect;

/// Derive a pointer by navigating arrays and struct fields within `addr`'s pointee.
///
/// Each index selects an element of the aggregate selected by the preceding index. Empty
/// indices leave the pointee unchanged. Offsets are measured in bytes in every address space.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    name = "gep",
    dialect = HirDialect,
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct GetElementPtr {
    #[operand]
    addr: AnyPointer,
    #[attr]
    indices: U32ArrayAttr,
    #[result]
    result: AnyPointer,
}

impl GetElementPtr {
    /// Return the byte offset from `addr` and the selected pointee type.
    pub fn get_element(&self) -> Result<(usize, Type), Report> {
        let pointer = self.addr().ty();
        let Some(pointee) = pointer.pointee() else {
            return Err(Report::msg(format!(
                "invalid hir.gep operand: expected a pointer, got '{pointer}'"
            )));
        };
        let mut current = pointee.clone();
        let mut offset = 0usize;
        for index in self.get_indices().iter().copied().map(|index| index as usize) {
            let (element_offset, element) = match &current {
                Type::Array(array) => {
                    if index >= array.len() {
                        return Err(Report::msg(format!(
                            "invalid hir.gep array index {index}: array has {} elements",
                            array.len()
                        )));
                    }
                    let element = array.element_type();
                    let stride = checked_size_in_bytes(element)?
                        .checked_next_multiple_of(element.min_alignment())
                        .ok_or_else(layout_overflow)?;
                    (stride.checked_mul(index).ok_or_else(layout_overflow)?, element.clone())
                }
                Type::Struct(structure) => {
                    let structure = structure.get();
                    let Some(field) = structure.fields().get(index) else {
                        return Err(Report::msg(format!(
                            "invalid hir.gep field index {index}: struct has {} fields",
                            structure.len()
                        )));
                    };
                    (field.offset as usize, field.ty.clone())
                }
                _ => {
                    return Err(Report::msg(format!(
                        "invalid hir.gep index {index}: cannot index non-aggregate type \
                         '{current}'"
                    )));
                }
            };
            offset = offset.checked_add(element_offset).ok_or_else(layout_overflow)?;
            current = element;
        }
        Ok((offset, current))
    }

    /// Return the byte offset from `addr` to the selected element.
    pub fn get_offset(&self) -> Result<usize, Report> {
        self.get_element().map(|(offset, _)| offset)
    }
}

impl InferTypeOpInterface for GetElementPtr {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let (_, pointee) = self.get_element()?;
        let Type::Ptr(pointer) = self.addr().ty() else {
            // `get_element` has already diagnosed non-pointer operands.
            unreachable!()
        };
        self.result_mut().set_type(Type::from(PointerType::new_with_address_space(
            pointee,
            pointer.addrspace(),
        )));
        Ok(())
    }
}

fn layout_overflow() -> Report {
    Report::msg("invalid hir.gep: byte layout offset overflows usize")
}

/// Array layouts in the type library use unchecked arithmetic. Compute their size here with
/// checked arithmetic so even malformed nested array types produce a diagnostic, not a panic.
fn checked_size_in_bytes(ty: &Type) -> Result<usize, Report> {
    match ty {
        Type::Array(array) => {
            if array.len() == 0 {
                return Ok(0);
            }
            let element = array.element_type();
            let size = checked_size_in_bytes(element)?;
            let stride = size
                .checked_next_multiple_of(element.min_alignment())
                .ok_or_else(layout_overflow)?;
            stride
                .checked_mul(array.len() - 1)
                .and_then(|padding| padding.checked_add(size))
                .ok_or_else(layout_overflow)
        }
        // These aggregate layouts store their already-computed byte size.
        Type::Struct(structure) => Ok(structure.size()),
        Type::Enum(enumeration) => Ok(enumeration.size_in_bytes()),
        _ => Ok(ty.size_in_bytes()),
    }
}

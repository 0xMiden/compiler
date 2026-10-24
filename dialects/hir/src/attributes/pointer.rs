use alloc::sync::Arc;
use core::fmt;

use midenc_hir::{
    AttrPrinter, PointerType, Type, attributes::InferAttributeValueType, derive::DialectAttribute,
};

use crate::HirDialect;

/// A constant address with its complete pointer type.
///
/// The address is interpreted in the stored type's address space. Changing type metadata does
/// not convert the address between byte and element units.
#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = HirDialect,
    implements(AttrPrinter)
)]
pub struct Pointer {
    addr: u32,
    /// Includes the outer address space and any nested pointer types.
    ty: PointerType,
}

impl Pointer {
    /// Create a constant with the given address bits and complete pointer type.
    pub fn new(addr: u32, ty: PointerType) -> Self {
        Self { addr, ty }
    }

    /// Returns the address bits without converting address units.
    pub fn addr(&self) -> u32 {
        self.addr
    }

    /// Returns the complete type, including the outer address space.
    pub fn pointer_type(&self) -> &PointerType {
        &self.ty
    }

    /// Returns the type of the value located at this address.
    pub fn pointee_type(&self) -> &Type {
        self.ty.pointee()
    }

    /// Change the pointee while preserving the outer address space and address bits.
    pub fn set_pointee_type(&mut self, ty: Type) {
        self.ty.pointee = ty;
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            addr: 0,
            ty: PointerType::new(Type::U8),
        }
    }
}

impl fmt::Display for Pointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.addr, f)
    }
}

impl midenc_hir::formatter::PrettyPrint for PointerAttr {
    fn render(&self) -> midenc_hir::formatter::Document {
        use midenc_hir::formatter::*;

        display(&self.value)
    }
}

impl AttrPrinter for PointerAttr {
    fn print(&self, printer: &mut midenc_hir::print::AsmPrinter<'_>) {
        printer.print_decimal_integer(self.value.addr());
    }
}

impl InferAttributeValueType for Pointer {
    fn infer_type() -> Type {
        Type::Ptr(Arc::new(PointerType::new(Type::U8)))
    }

    fn infer_type_from_value(&self) -> Type {
        Type::from(self.pointer_type().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_constant_default_has_one_pointer_layer() {
        let pointer = Pointer::default();
        assert_eq!(pointer.addr(), 0);
        assert_eq!(pointer.pointee_type(), &Type::U8);
        assert_eq!(pointer.infer_type_from_value(), Pointer::infer_type());
    }
    #[test]
    fn pointer_constant_pointee_updates_preserve_outer_space_and_address() {
        for space in [midenc_hir::AddressSpace::Byte, midenc_hir::AddressSpace::Element] {
            let mut pointer =
                Pointer::new(u32::MAX, PointerType::new_with_address_space(Type::U32, space));
            let nested = Type::from(PointerType::new_with_address_space(
                Type::U8,
                midenc_hir::AddressSpace::Element,
            ));
            pointer.set_pointee_type(nested.clone());
            assert_eq!(pointer.addr(), u32::MAX);
            assert_eq!(pointer.pointer_type().addrspace(), space);
            assert_eq!(pointer.pointee_type(), &nested);
            assert_eq!(pointer.infer_type_from_value(), Type::from(pointer.pointer_type().clone()));
        }
    }
}

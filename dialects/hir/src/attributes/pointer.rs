use alloc::sync::Arc;
use core::fmt;

use midenc_hir::{
    AttrPrinter, PointerType, Type, attributes::InferAttributeValueType, derive::DialectAttribute,
};

use crate::HirDialect;

/// Represents a constant pointer value
#[derive(DialectAttribute, Debug, Clone, PartialEq, Eq, Hash)]
#[attribute(
    dialect = HirDialect,
    implements(AttrPrinter)
)]
pub struct Pointer {
    addr: u32,
    /// The pointee type
    ty: Type,
}

impl Pointer {
    pub fn new(addr: u32, ty: Type) -> Self {
        Self { addr, ty }
    }

    pub fn addr(&self) -> u32 {
        self.addr
    }

    pub fn pointee_type(&self) -> &Type {
        &self.ty
    }

    pub fn set_pointee_type(&mut self, ty: Type) {
        self.ty = ty;
    }
}

impl Default for Pointer {
    fn default() -> Self {
        Self {
            addr: 0,
            ty: Self::infer_type(),
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
        Type::Ptr(Arc::new(PointerType::new(self.pointee_type().clone())))
    }
}

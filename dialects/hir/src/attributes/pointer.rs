use alloc::boxed::Box;

use midenc_hir::{AttributeValue, Immediate, Type, formatter};

/// Represents a constant pointer value
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PointerAttr {
    addr: Immediate,
    /// The pointee type
    ty: Type,
}

impl PointerAttr {
    pub fn new(addr: Immediate, ty: Type) -> Self {
        Self { addr, ty }
    }

    pub fn addr(&self) -> &Immediate {
        &self.addr
    }

    pub fn pointee_type(&self) -> &Type {
        &self.ty
    }

    pub fn set_pointee_type(&mut self, ty: Type) {
        self.ty = ty;
    }
}

impl formatter::PrettyPrint for PointerAttr {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        display(self.addr)
    }
}

impl AttributeValue for PointerAttr {
    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn core::any::Any {
        self
    }

    fn clone_value(&self) -> Box<dyn AttributeValue> {
        Box::new(self.clone())
    }
}

use alloc::boxed::Box;

use midenc_hir2::{formatter, AttributeValue, Felt, Immediate, Type};

/// Represents the constant value of the 'hir.poison' operation
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PoisonAttr {
    /// The value type that was poisoned
    ty: Type,
}

impl PoisonAttr {
    pub fn new(ty: Type) -> Self {
        Self { ty }
    }

    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn into_type(self) -> Type {
        self.ty
    }

    pub fn as_immediate(&self) -> Result<Immediate, Type> {
        Ok(match &self.ty {
            Type::I1 => Immediate::I1(false),
            Type::U8 => Immediate::U8(0xde),
            Type::I8 => Immediate::I8(0xdeu8 as i8),
            Type::U16 => Immediate::U16(0xdead),
            Type::I16 => Immediate::I16(0xdeadu16 as i16),
            Type::U32 => Immediate::U32(0xdeadc0de),
            Type::I32 => Immediate::I32(0xdeadc0deu32 as i32),
            Type::U64 => Immediate::U64(0xdeadc0dedeadc0de),
            Type::I64 => Immediate::I64(0xdeadc0dedeadc0deu64 as i64),
            Type::Felt => Immediate::Felt(Felt::new(0xdeadc0de)),
            Type::U128 => Immediate::U128(0xdeadc0dedeadc0dedeadc0dedeadc0de),
            Type::I128 => Immediate::I128(0xdeadc0dedeadc0dedeadc0dedeadc0deu128 as i128),
            // We emit a pointer that can never refer to a valid object in memory
            Type::Ptr(_) => Immediate::U32(u32::MAX),
            ty => return Err(ty.clone()),
        })
    }
}

impl formatter::PrettyPrint for PoisonAttr {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        display(&self.ty)
    }
}

impl AttributeValue for PoisonAttr {
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

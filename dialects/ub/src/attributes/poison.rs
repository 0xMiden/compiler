use midenc_hir::{
    AttrPrinter, Felt, Immediate, Type, derive::DialectAttribute, formatter, print::AsmPrinter,
};

use crate::UndefinedBehaviorDialect;

/// Represents the constant value of the 'hir.poison' operation
#[derive(DialectAttribute)]
#[attribute(
    dialect = UndefinedBehaviorDialect,
    remote = "Type",
    default = "poison_value",
    implements(AttrPrinter)
)]
#[allow(unused)]
struct Poison;

const fn poison_value() -> Type {
    Type::Unknown
}

impl PoisonAttr {
    pub fn as_immediate(&self) -> Option<Immediate> {
        Some(match &self.value {
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
            _ty => return None,
        })
    }
}

impl formatter::PrettyPrint for PoisonAttr {
    fn render(&self) -> formatter::Document {
        use formatter::*;

        display(&self.value)
    }
}

impl AttrPrinter for PoisonAttr {
    fn print(&self, printer: &mut AsmPrinter<'_>) {
        printer.print_type(&self.value);
    }
}

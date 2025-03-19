use midenc_hir::{derive::operation, effects::*, traits::*, *};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    implements(OpPrinter, MemoryEffectOpInterface)
)]
pub struct Assert {
    #[operand]
    value: Bool,
    #[attr(hidden)]
    #[default]
    code: u32,
}

impl EffectOpInterface<MemoryEffect> for Assert {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

impl OpPrinter for Assert {
    fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> formatter::Document {
        use formatter::*;

        let doc = display(self.op.name()) + const_text(" ") + display(self.value().as_value_ref());
        let code = *self.code();
        if code == 0 {
            doc + const_text(";")
        } else {
            doc + const_text(" #[code = ") + display(code) + const_text("];")
        }
    }
}

#[operation(
    dialect = HirDialect,
    implements(OpPrinter, MemoryEffectOpInterface)
)]
pub struct Assertz {
    #[operand]
    value: Bool,
    #[attr(hidden)]
    #[default]
    code: u32,
}

impl EffectOpInterface<MemoryEffect> for Assertz {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

impl OpPrinter for Assertz {
    fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> formatter::Document {
        use formatter::*;

        let doc = display(self.op.name()) + const_text(" ") + display(self.value().as_value_ref());
        let code = *self.code();
        if code == 0 {
            doc + const_text(";")
        } else {
            doc + const_text(" #[code = ") + display(code) + const_text("];")
        }
    }
}

#[operation(
    dialect = HirDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(MemoryEffectOpInterface)
)]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
}

impl EffectOpInterface<MemoryEffect> for AssertEq {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

use midenc_hir2::{derive::operation, effects::*, traits::*, *};

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

#[operation(
    dialect = HirDialect,
    implements(OpPrinter, MemoryEffectOpInterface)
)]
pub struct AssertEqImm {
    #[operand]
    lhs: AnyInteger,
    #[attr(hidden)]
    rhs: Immediate,
}

impl EffectOpInterface<MemoryEffect> for AssertEqImm {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

impl OpPrinter for AssertEqImm {
    fn print(&self, _flags: &OpPrintingFlags, _context: &Context) -> formatter::Document {
        use formatter::*;

        display(self.op.name())
            + const_text(" ")
            + display(self.lhs().as_value_ref())
            + const_text(", ")
            + display(self.rhs())
            + const_text(";")
    }
}

#[operation(
    dialect = HirDialect,
    traits(Terminator),
    implements(MemoryEffectOpInterface)
)]
pub struct Unreachable {}

impl EffectOpInterface<MemoryEffect> for Unreachable {
    fn effects(&self) -> EffectIterator<MemoryEffect> {
        EffectIterator::from_smallvec(smallvec![EffectInstance::new(MemoryEffect::Write)])
    }
}

#[operation(
    dialect = HirDialect,
    traits(ConstantLike),
    implements(InferTypeOpInterface, Foldable)
)]
pub struct Poison {
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyType,
}

impl Foldable for Poison {
    fn fold(&self, _results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        FoldResult::InPlace
    }

    fn fold_with(
        &self,
        _operands: &[Option<std::prelude::v1::Box<dyn AttributeValue>>],
        _results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        FoldResult::InPlace
    }
}

impl InferTypeOpInterface for Poison {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let poison_ty = self.ty().clone();
        self.result_mut().set_type(poison_ty);
        Ok(())
    }
}

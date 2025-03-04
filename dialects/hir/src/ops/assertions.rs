use midenc_hir2::{derive::operation, traits::*, *};

use crate::HirDialect;

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryWrite),
    implements(OpPrinter)
)]
pub struct Assert {
    #[operand]
    value: Bool,
    #[attr(hidden)]
    #[default]
    code: u32,
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
    traits(HasSideEffects, MemoryWrite),
    implements(OpPrinter)
)]
pub struct Assertz {
    #[operand]
    value: Bool,
    #[attr(hidden)]
    #[default]
    code: u32,
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
    traits(BinaryOp, HasSideEffects, MemoryWrite, Commutative, SameTypeOperands)
)]
pub struct AssertEq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
}

#[operation(
    dialect = HirDialect,
    traits(HasSideEffects, MemoryWrite),
    implements(OpPrinter)
)]
pub struct AssertEqImm {
    #[operand]
    lhs: AnyInteger,
    #[attr(hidden)]
    rhs: Immediate,
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
    traits(HasSideEffects, Terminator)
)]
pub struct Unreachable {}

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

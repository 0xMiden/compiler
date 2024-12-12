use crate::{derive::operation, dialects::test::TestDialect, traits::*, *};

/// Two's complement sum
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface)
)]
pub struct Add {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Add {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Two's complement product
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface)
)]
pub struct Mul {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: Overflow,
}

impl InferTypeOpInterface for Mul {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

/// Bitwise shift-left
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[operation(
    dialect = TestDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface)
)]
pub struct Shl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Shl {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let lhs = self.lhs().ty().clone();
        self.result_mut().set_type(lhs);
        Ok(())
    }
}

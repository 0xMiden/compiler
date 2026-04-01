use crate::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    dialects::{builtin::attributes::OverflowAttr, test::TestDialect},
    effects::*,
    traits::*,
    *,
};

macro_rules! infer_return_ty_for_binary_op {
    ($Op:ty) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                let lhs = self.lhs().ty().clone();
                self.result_mut().set_type(lhs);
                Ok(())
            }
        }
    };

    ($Op:ty as $manually_specified_ty:expr) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                self.result_mut().set_type($manually_specified_ty);
                Ok(())
            }
        }
    };
}

/// Two's complement sum
#[derive(OpParser, OpPrinter, EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Add {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: OverflowAttr,
}

infer_return_ty_for_binary_op!(Add);

/// Two's complement product
#[derive(OpParser, OpPrinter, EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Mul {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: AnyInteger,
    #[attr]
    overflow: OverflowAttr,
}

infer_return_ty_for_binary_op!(Mul);

/// Bitwise shift-left
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[derive(OpParser, OpPrinter, EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Shl {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    shift: UInt32,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_binary_op!(Shl);

/// Equality comparison
#[derive(OpParser, OpPrinter, EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Eq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Eq as Type::I1);

/// Inequality comparison
#[derive(OpParser, OpPrinter, EffectOpInterface)]
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Neq {
    #[operand]
    lhs: AnyInteger,
    #[operand]
    rhs: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_binary_op!(Neq as Type::I1);

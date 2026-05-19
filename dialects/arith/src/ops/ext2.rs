use midenc_hir::{
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
    effects::MemoryEffectOpInterface,
    traits::*,
    *,
};

use crate::ArithDialect;

macro_rules! infer_ext2_result_types {
    ($Op:ty) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                self.result0_mut().set_type(Type::Felt);
                self.result1_mut().set_type(Type::Felt);
                Ok(())
            }
        }
    };
}

/// Extension-field addition over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Add {
    #[operand]
    lhs0: IntFelt,
    #[operand]
    lhs1: IntFelt,
    #[operand]
    rhs0: IntFelt,
    #[operand]
    rhs1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Add);

/// Extension-field subtraction over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Sub {
    #[operand]
    lhs0: IntFelt,
    #[operand]
    lhs1: IntFelt,
    #[operand]
    rhs0: IntFelt,
    #[operand]
    rhs1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Sub);

/// Extension-field multiplication over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Mul {
    #[operand]
    lhs0: IntFelt,
    #[operand]
    lhs1: IntFelt,
    #[operand]
    rhs0: IntFelt,
    #[operand]
    rhs1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Mul);

/// Extension-field division over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Div {
    #[operand]
    lhs0: IntFelt,
    #[operand]
    lhs1: IntFelt,
    #[operand]
    rhs0: IntFelt,
    #[operand]
    rhs1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Div);

/// Extension-field negation over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Neg {
    #[operand]
    operand0: IntFelt,
    #[operand]
    operand1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Neg);

/// Extension-field inversion over two felt limbs.
#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Ext2Inv {
    #[operand]
    operand0: IntFelt,
    #[operand]
    operand1: IntFelt,
    #[result]
    result0: IntFelt,
    #[result]
    result1: IntFelt,
}

infer_ext2_result_types!(Ext2Inv);

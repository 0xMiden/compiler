use midenc_hir2::{derive::operation, effects::MemoryEffectOpInterface, traits::*, *};

use crate::*;

macro_rules! infer_return_ty_for_unary_op {
    ($Op:ty) => {
        impl InferTypeOpInterface for $Op {
            fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                let lhs = self.operand().ty().clone();
                self.result_mut().set_type(lhs);
                Ok(())
            }
        }
    };

    ($Op:ty as $manually_specified_ty:expr) => {
        paste::paste! {
            impl InferTypeOpInterface for $Op {
                fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
                    self.result_mut().set_type($manually_specified_ty);
                    Ok(())
                }
            }
        }
    };
}

/// Increment
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Incr {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Incr);
has_no_effects!(Incr);

/// Negation
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Neg {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Neg);
has_no_effects!(Neg);

/// Modular inverse
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Inv {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_unary_op!(Inv);
has_no_effects!(Inv);

/// log2(operand)
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Ilog2 {
    #[operand]
    operand: IntFelt,
    #[result]
    result: IntFelt,
}

infer_return_ty_for_unary_op!(Ilog2);
has_no_effects!(Ilog2);

/// pow2(operand)
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Pow2 {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Pow2);
has_no_effects!(Pow2);

/// Logical NOT
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)

    )]
pub struct Not {
    #[operand]
    operand: Bool,
    #[result]
    result: Bool,
}

infer_return_ty_for_unary_op!(Not);
has_no_effects!(Not);

/// Bitwise NOT
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp, SameOperandsAndResultType),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Bnot {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: AnyInteger,
}

infer_return_ty_for_unary_op!(Bnot);
has_no_effects!(Bnot);

/// is_odd(operand)
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct IsOdd {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: Bool,
}

infer_return_ty_for_unary_op!(IsOdd as Type::I1);
has_no_effects!(IsOdd);

/// Count of non-zero bits (population count)
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Popcnt {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Popcnt as Type::U32);
has_no_effects!(Popcnt);

/// Count Leading Zeros
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Clz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Clz as Type::U32);
has_no_effects!(Clz);

/// Count Trailing Zeros
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Ctz {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Ctz as Type::U32);
has_no_effects!(Ctz);

/// Count Leading Ones
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Clo {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Clo as Type::U32);
has_no_effects!(Clo);

/// Count Trailing Ones
#[operation (
        dialect = ArithDialect,
        traits(UnaryOp),
        implements(InferTypeOpInterface, MemoryEffectOpInterface)
    )]
pub struct Cto {
    #[operand]
    operand: AnyInteger,
    #[result]
    result: UInt32,
}

infer_return_ty_for_unary_op!(Cto as Type::U32);
has_no_effects!(Cto);

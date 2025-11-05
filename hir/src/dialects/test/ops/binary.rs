use crate::{derive::operation, dialects::test::TestDialect, effects::*, traits::*, *};

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

macro_rules! has_no_effects {
    ($Op:ty) => {
        impl EffectOpInterface<MemoryEffect> for $Op {
            fn has_no_effect(&self) -> bool {
                true
            }

            fn effects(&self) -> EffectIterator<::midenc_hir::effects::MemoryEffect> {
                EffectIterator::from_smallvec(::midenc_hir::smallvec![])
            }
        }
    };
}

/// Two's complement sum
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands, SameOperandsAndResultType),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
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

infer_return_ty_for_binary_op!(Add);
has_no_effects!(Add);

/// Two's complement product
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
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

infer_return_ty_for_binary_op!(Mul);
has_no_effects!(Mul);

/// Bitwise shift-left
///
/// Shifts larger than the bitwidth of the value will be wrapped to zero.
#[operation(
    dialect = TestDialect,
    traits(BinaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
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
has_no_effects!(Shl);

/// Equality comparison
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
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
has_no_effects!(Eq);

/// Inequality comparison
#[operation(
    dialect = TestDialect,
    traits(BinaryOp, Commutative, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
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
has_no_effects!(Neq);

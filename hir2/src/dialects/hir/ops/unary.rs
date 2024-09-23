use crate::{dialects::hir::HirDialect, traits::*, *};

macro_rules! derive_unary_op {
    ($Op:ident) => {
        derive! {
            pub struct $Op: Op {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                operand: OpOperandRef,
                #[result]
                result: OpResultRef,
            }

            derives UnaryOp;
        }
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive! {
            pub struct $Op: Op {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                operand: OpOperandRef,
                #[result]
                result: OpResultRef,
            }

            derives UnaryOp, $OpTrait $(, $OpTraitRest)*;
        }
    };
}

macro_rules! derive_unary_logical_op {
    ($Op:ident) => {
        derive_unary_op!($Op derives SameOperandsAndResultType);
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_unary_op!($Op derives SameOperandsAndResultType, $OpTrait $(, $OpTraitRest)*);
    };
}

macro_rules! derive_unary_bitwise_op {
    ($Op:ident) => {
        derive_unary_op!($Op derives SameOperandsAndResultType);
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_unary_op!($Op derives SameOperandsAndResultType, $OpTrait $(, $OpTraitRest)*);
    };
}

derive_unary_op!(Neg derives SameOperandsAndResultType);
derive_unary_op!(Inv derives SameOperandsAndResultType);
derive_unary_op!(Incr derives SameOperandsAndResultType);
derive_unary_op!(Ilog2 derives SameOperandsAndResultType);
derive_unary_op!(Pow2 derives SameOperandsAndResultType);

derive_unary_logical_op!(Not);
derive_unary_logical_op!(IsOdd);

derive_unary_bitwise_op!(Bnot);
derive_unary_bitwise_op!(Popcnt);
derive_unary_bitwise_op!(Clz);
derive_unary_bitwise_op!(Ctz);
derive_unary_bitwise_op!(Clo);
derive_unary_bitwise_op!(Cto);

use crate::{dialects::hir::HirDialect, traits::*, *};

macro_rules! derive_binary_op_with_overflow {
    ($Op:ident) => {
        derive! {
            pub struct $Op: Op implements BinaryOp {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
                #[attr]
                overflow: Overflow,
            }
        }
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive! {
            pub struct $Op: Op implements BinaryOp, $OpTrait $(, $OpTraitRest)* {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
                #[attr]
                overflow: Overflow,
            }
        }
    };
}

macro_rules! derive_binary_op {
    ($Op:ident) => {
        derive! {
            pub struct $Op: Op implements BinaryOp {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
            }
        }
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive! {
            pub struct $Op: Op implements BinaryOp, $OpTrait $(, $OpTraitRest)* {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
            }
        }
    };
}

macro_rules! derive_binary_logical_op {
    ($Op:ident) => {
        derive_binary_op!($Op implements SameTypeOperands, SameOperandsAndResultType, Commutative);
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op implements SameTypeOperands, SameOperandsAndResultType, Commutative, $OpTrait $(, $OpTraitRest)*);
    };
}

macro_rules! derive_binary_bitwise_op {
    ($Op:ident) => {
        derive_binary_op!($Op implements SameTypeOperands, SameOperandsAndResultType);
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op implements SameTypeOperands, SameOperandsAndResultType, $OpTrait $(, $OpTraitRest)*);
    };
}

macro_rules! derive_binary_comparison_op {
    ($Op:ident) => {
        derive_binary_op!($Op implements SameTypeOperands);
    };

    ($Op:ident implements $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op implements SameTypeOperands, $OpTrait $(, $OpTraitRest)*);
    };
}

derive_binary_op_with_overflow!(Add implements Commutative, SameTypeOperands);
derive_binary_op_with_overflow!(Sub implements SameTypeOperands);
derive_binary_op_with_overflow!(Mul implements Commutative, SameTypeOperands);
derive_binary_op_with_overflow!(Exp);

derive_binary_op!(Div implements SameTypeOperands, SameOperandsAndResultType);
derive_binary_op!(Mod implements SameTypeOperands, SameOperandsAndResultType);
derive_binary_op!(DivMod implements SameTypeOperands, SameOperandsAndResultType);

derive_binary_logical_op!(And);
derive_binary_logical_op!(Or);
derive_binary_logical_op!(Xor);

derive_binary_bitwise_op!(Band implements Commutative);
derive_binary_bitwise_op!(Bor implements Commutative);
derive_binary_bitwise_op!(Bxor implements Commutative);
derive_binary_op!(Shl);
derive_binary_op!(Shr);
derive_binary_op!(Rotl);
derive_binary_op!(Rotr);

derive_binary_comparison_op!(Eq implements Commutative);
derive_binary_comparison_op!(Neq implements Commutative);
derive_binary_comparison_op!(Gt);
derive_binary_comparison_op!(Gte);
derive_binary_comparison_op!(Lt);
derive_binary_comparison_op!(Lte);
derive_binary_comparison_op!(Min implements Commutative);
derive_binary_comparison_op!(Max implements Commutative);

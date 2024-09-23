use crate::{dialects::hir::HirDialect, traits::*, *};

macro_rules! derive_binary_op_with_overflow {
    ($Op:ident) => {
        derive! {
            pub struct $Op: Op {
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

            derives BinaryOp;
        }
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive! {
            pub struct $Op: Op {
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

            derives BinaryOp, $OpTrait $(, $OpTraitRest)*;
        }
    };
}

macro_rules! derive_binary_op {
    ($Op:ident) => {
        derive! {
            pub struct $Op: Op {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
            }

            derives BinaryOp;
        }
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive! {
            pub struct $Op: Op {
                #[dialect]
                dialect: HirDialect,
                #[operand]
                lhs: OpOperandRef,
                #[operand]
                rhs: OpOperandRef,
                #[result]
                result: OpResultRef,
            }

            derives BinaryOp, $OpTrait $(, $OpTraitRest)*;
        }
    };
}

macro_rules! derive_binary_logical_op {
    ($Op:ident) => {
        derive_binary_op!($Op derives SameTypeOperands, SameOperandsAndResultType, Commutative);
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op derives SameTypeOperands, SameOperandsAndResultType, Commutative, $OpTrait $(, $OpTraitRest)*);
    };
}

macro_rules! derive_binary_bitwise_op {
    ($Op:ident) => {
        derive_binary_op!($Op derives SameTypeOperands, SameOperandsAndResultType);
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op derives SameTypeOperands, SameOperandsAndResultType, $OpTrait $(, $OpTraitRest)*);
    };
}

macro_rules! derive_binary_comparison_op {
    ($Op:ident) => {
        derive_binary_op!($Op derives SameTypeOperands);
    };

    ($Op:ident derives $OpTrait:ident $(, $OpTraitRest:ident)*) => {
        derive_binary_op!($Op derives SameTypeOperands, $OpTrait $(, $OpTraitRest)*);
    };
}

derive_binary_op_with_overflow!(Add derives Commutative, SameTypeOperands);
derive_binary_op_with_overflow!(Sub derives SameTypeOperands);
derive_binary_op_with_overflow!(Mul derives Commutative, SameTypeOperands);
derive_binary_op_with_overflow!(Exp);

derive_binary_op!(Div derives SameTypeOperands, SameOperandsAndResultType);
derive_binary_op!(Mod derives SameTypeOperands, SameOperandsAndResultType);
derive_binary_op!(DivMod derives SameTypeOperands, SameOperandsAndResultType);

derive_binary_logical_op!(And);
derive_binary_logical_op!(Or);
derive_binary_logical_op!(Xor);

derive_binary_bitwise_op!(Band derives Commutative);
derive_binary_bitwise_op!(Bor derives Commutative);
derive_binary_bitwise_op!(Bxor derives Commutative);
derive_binary_op!(Shl);
derive_binary_op!(Shr);
derive_binary_op!(Rotl);
derive_binary_op!(Rotr);

derive_binary_comparison_op!(Eq derives Commutative);
derive_binary_comparison_op!(Neq derives Commutative);
derive_binary_comparison_op!(Gt);
derive_binary_comparison_op!(Gte);
derive_binary_comparison_op!(Lt);
derive_binary_comparison_op!(Lte);
derive_binary_comparison_op!(Min derives Commutative);
derive_binary_comparison_op!(Max derives Commutative);

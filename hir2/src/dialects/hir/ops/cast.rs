use crate::{dialects::hir::HirDialect, traits::*, *};

/*
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CastKind {
    /// Reinterpret the bits of the operand as the target type, without any consideration for
    /// the original meaning of those bits.
    ///
    /// For example, transmuting `u32::MAX` to `i32`, produces a value of `-1`, because the input
    /// value overflows when interpreted as a signed integer.
    Transmute,
    /// Like `Transmute`, but the input operand is checked to verify that it is a valid value
    /// of both the source and target types.
    ///
    /// For example, a checked cast of `u32::MAX` to `i32` would assert, because the input value
    /// cannot be represented as an `i32` due to overflow.
    Checked,
    /// Convert the input value to the target type, by zero-extending the value to the target
    /// bitwidth. A cast of this type must be a widening cast, i.e. from a smaller bitwidth to
    /// a larger one.
    Zext,
    /// Convert the input value to the target type, by sign-extending the value to the target
    /// bitwidth. A cast of this type must be a widening cast, i.e. from a smaller bitwidth to
    /// a larger one.
    Sext,
    /// Convert the input value to the target type, by truncating the excess bits. A cast of this
    /// type must be a narrowing cast, i.e. from a larger bitwidth to a smaller one.
    Trunc,
}
 */

derive! {
    pub struct PtrToInt : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct IntToPtr : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct Cast : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct Bitcast : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct Trunc : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct Zext : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

derive! {
    pub struct Sext : Op implements UnaryOp {
        #[dialect]
        dialect: HirDialect,
        #[attr]
        ty: Type,
        #[operand]
        operand: OpOperand,
        #[result]
        result: OpResult,
    }
}

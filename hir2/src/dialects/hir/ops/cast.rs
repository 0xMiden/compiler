use midenc_hir_macros::operation;

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

#[operation(
     dialect = HirDialect,
     traits(UnaryOp)
 )]
pub struct PtrToInt {
    #[operand]
    operand: AnyPointer,
    #[attr]
    ty: Type,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for PtrToInt {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct IntToPtr {
    #[operand]
    operand: AnyInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for IntToPtr {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct Cast {
    #[operand]
    operand: AnyInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Cast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct Bitcast {
    #[operand]
    operand: AnyPointerOrInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnyPointerOrInteger,
}

impl InferTypeOpInterface for Bitcast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct Trunc {
    #[operand]
    operand: AnyInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Trunc {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct Zext {
    #[operand]
    operand: AnyUnsignedInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnyUnsignedInteger,
}

impl InferTypeOpInterface for Zext {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp)
)]
pub struct Sext {
    #[operand]
    operand: AnySignedInteger,
    #[attr]
    ty: Type,
    #[result]
    result: AnySignedInteger,
}

impl InferTypeOpInterface for Sext {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

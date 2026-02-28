use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpPrinter, operation},
    dialects::builtin::attributes::{I32Attr, TypeAttr, U32Attr},
    effects::MemoryEffectOpInterface,
    matchers::Matcher,
    traits::*,
    *,
};

use crate::{HirDialect, PointerAttr};

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

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
 )]
pub struct PtrToInt {
    #[operand]
    operand: AnyPointer,
    #[attr(hidden)]
    ty: TypeAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for PtrToInt {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for PtrToInt {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(value) =
            matchers::foldable_operand_of::<PointerAttr>().matches(&self.operand().as_operand_ref())
        {
            let input = value.borrow();
            // Support folding just pointer -> 32-bit integer types for now
            let output = match &*self.get_ty() {
                Type::U32 => input
                    .context_rc()
                    .create_attribute::<U32Attr, _>(input.addr())
                    .as_attribute_ref(),
                Type::I32 => input
                    .context_rc()
                    .create_attribute::<I32Attr, _>(input.addr() as i32)
                    .as_attribute_ref(),
                _ => return FoldResult::Failed,
            };
            results.push(OpFoldResult::Attribute(output));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_ref().and_then(|o| o.try_downcast::<PointerAttr>().ok())
        {
            let input = value.borrow();
            // Support folding just pointer -> 32-bit integer types for now
            let output = match &*self.get_ty() {
                Type::U32 => input
                    .context_rc()
                    .create_attribute::<U32Attr, _>(input.addr())
                    .as_attribute_ref(),
                Type::I32 => input
                    .context_rc()
                    .create_attribute::<I32Attr, _>(input.addr() as i32)
                    .as_attribute_ref(),
                _ => return FoldResult::Failed,
            };
            results.push(OpFoldResult::Attribute(output));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }
}

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct IntToPtr {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    ty: TypeAttr,
    #[result]
    result: AnyPointer,
}

impl InferTypeOpInterface for IntToPtr {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for IntToPtr {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(value) = matchers::foldable_operand_of_trait::<dyn IntegerLikeAttr>()
            .matches(&self.operand().as_operand_ref())
        {
            results.push(OpFoldResult::Attribute(value));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        let Some(attr) = operands[0].as_ref() else {
            return FoldResult::Failed;
        };

        let attr_borrowed = attr.borrow();
        if let Some(integer_like) = attr_borrowed.as_attr().as_trait::<dyn IntegerLikeAttr>()
            && let Some(addr) = integer_like.as_immediate().as_u32()
        {
            let ty = self.get_ty().clone();
            let ptr = crate::attributes::Pointer::new(addr, ty);
            let attr = integer_like.context_rc().create_attribute::<PointerAttr, _>(ptr);
            results.push(OpFoldResult::Attribute(attr));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }
}

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, OpPrinter)
)]
pub struct Cast {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    ty: TypeAttr,
    #[result]
    result: AnyInteger,
}

impl InferTypeOpInterface for Cast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[derive(EffectOpInterface, OpPrinter)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable, OpPrinter)
)]
pub struct Bitcast {
    #[operand]
    operand: AnyPointerOrInteger,
    #[attr(hidden)]
    ty: TypeAttr,
    #[result]
    result: AnyPointerOrInteger,
}

impl InferTypeOpInterface for Bitcast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.get_ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Bitcast {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(attr) = matchers::foldable_operand().matches(&self.operand().as_operand_ref()) {
            let attr_borrowed = attr.borrow();
            if attr_borrowed.as_attr().implements::<dyn IntegerLikeAttr>()
                || attr_borrowed.is::<PointerAttr>()
            {
                // Lean on materialize_constant to handle the conversion details
                results.push(OpFoldResult::Attribute(attr));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<AttributeRef>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        let Some(attr) = operands[0].as_ref() else {
            return FoldResult::Failed;
        };

        let attr_borrowed = attr.borrow();
        if attr_borrowed.as_attr().implements::<dyn IntegerLikeAttr>()
            || attr_borrowed.is::<PointerAttr>()
        {
            // Lean on materialize_constant to handle the conversion details
            results.push(OpFoldResult::Attribute(*attr));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }
}

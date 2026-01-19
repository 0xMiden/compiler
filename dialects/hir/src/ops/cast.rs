use alloc::boxed::Box;

use midenc_hir::{
    derive::operation, effects::MemoryEffectOpInterface, matchers::Matcher, traits::*, *,
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

#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
 )]
pub struct PtrToInt {
    #[operand]
    operand: AnyPointer,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyInteger,
}

has_no_effects!(PtrToInt);

impl InferTypeOpInterface for PtrToInt {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
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
            // Support folding just pointer -> 32-bit integer types for now
            let value = match self.ty() {
                Type::U32 => value.addr().as_u32().map(Immediate::U32),
                Type::I32 => value.addr().as_u32().map(|v| Immediate::I32(v as i32)),
                _ => return FoldResult::Failed,
            };
            if let Some(value) = value {
                results.push(OpFoldResult::Attribute(Box::new(value)));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<PointerAttr>()) {
            // Support folding just pointer -> 32-bit integer types for now
            let value = match self.ty() {
                Type::U32 => value.addr().as_u32().map(Immediate::U32),
                Type::I32 => value.addr().as_u32().map(|v| Immediate::I32(v as i32)),
                _ => return FoldResult::Failed,
            };
            if let Some(value) = value {
                results.push(OpFoldResult::Attribute(Box::new(value)));
                return FoldResult::Ok(());
            }
        }
        FoldResult::Failed
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct IntToPtr {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyPointer,
}

has_no_effects!(IntToPtr);

impl InferTypeOpInterface for IntToPtr {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for IntToPtr {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.operand().as_operand_ref())
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
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            let attr = PointerAttr::new(*value, Type::from(PointerType::new(self.ty().clone())));
            results.push(OpFoldResult::Attribute(Box::new(attr)));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Cast {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyInteger,
}

has_no_effects!(Cast);

impl InferTypeOpInterface for Cast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Bitcast {
    #[operand]
    operand: AnyPointerOrInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyPointerOrInteger,
}

has_no_effects!(Bitcast);

impl InferTypeOpInterface for Bitcast {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Bitcast {
    #[inline]
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(value) = matchers::foldable_operand().matches(&self.operand().as_operand_ref())
            && (value.is::<Immediate>() || value.is::<PointerAttr>())
        {
            // Lean on materialize_constant to handle the conversion details
            results.push(OpFoldResult::Attribute(value));
            return FoldResult::Ok(());
        }

        FoldResult::Failed
    }

    #[inline(always)]
    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) =
            operands[0].as_deref().filter(|o| o.is::<Immediate>() || o.is::<PointerAttr>())
        {
            results.push(OpFoldResult::Attribute(value.clone_value()));
            FoldResult::Ok(())
        } else {
            FoldResult::Failed
        }
    }
}

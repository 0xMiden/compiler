use midenc_hir::{
    attributes::IntegerLikeAttr,
    derive::{EffectOpInterface, OpParser, OpPrinter, operation},
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

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp, TransparentCast),
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
        if let Some(value) =
            operands[0].as_ref().and_then(|o| o.try_downcast_attr::<PointerAttr>().ok())
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

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp, TransparentCast),
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

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp),
    implements(
        InferTypeOpInterface,
        MemoryEffectOpInterface,
        OperandRangeRequirementOpInterface,
        CheckedCastOpInterface,
        OpPrinter
    )
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

impl OperandRangeRequirementOpInterface for Cast {
    fn operand_range_requirement(&self, _operand_index: usize) -> OperandRangeRequirement {
        // Casts may refine their result via `CheckedCastOpInterface`, but they are
        // not themselves semantic consumers of a range-constrained value.
        OperandRangeRequirement::None
    }
}

impl CheckedCastOpInterface for Cast {
    fn checked_cast_refinement(&self, result: ValueRef) -> Option<ValueRangeRefinement> {
        let cast_result = self.result().as_value_ref();
        if cast_result != result {
            return None;
        }

        let input = self.operand().as_value_ref();
        if input.borrow().ty() != &Type::Felt {
            return None;
        }

        Some(ValueRangeRefinement {
            input,
            result: cast_result,
            constraint: ValueRangeConstraint::from_type(self.result().ty())?,
        })
    }
}

#[derive(EffectOpInterface, OpPrinter, OpParser)]
#[operation(
    dialect = HirDialect,
    traits(UnaryOp, TransparentCast),
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

#[cfg(test)]
mod tests {
    use alloc::{format, vec::Vec};

    use midenc_hir::{
        AddressSpace, Op, PointerType, Report, SourceSpan, Type, ValueRef,
        dialects::builtin::BuiltinOpBuilder, testing::Test,
    };

    use crate::HirOpBuilder;

    #[derive(Copy, Clone, Debug)]
    enum CastOp {
        PtrToInt,
        IntToPtr,
        Bitcast,
    }

    /// Verify a function that casts its argument, of type `from`, to `to` with `cast`.
    fn verify_cast(cast: CastOp, from: Type, to: Type) -> Result<(), Report> {
        let span = SourceSpan::UNKNOWN;
        let mut test = Test::new("verify_cast", &[from], &[]);
        {
            let mut builder = test.function_builder();
            let arg = builder.entry_block().borrow().arguments()[0] as ValueRef;
            match cast {
                CastOp::PtrToInt => builder.ptrtoint(arg, to, span),
                CastOp::IntToPtr => builder.inttoptr(arg, to, span),
                CastOp::Bitcast => builder.bitcast(arg, to, span),
            }
            .unwrap();
            builder.ret(None, span).unwrap();
        }
        test.function().borrow().as_operation().recursively_verify()
    }

    fn pointer(pointee: Type) -> Type {
        Type::from(PointerType::new_with_address_space(pointee, AddressSpace::Element))
    }

    /// The three casts are `TransparentCast`s on every shape the frontends build them in.
    #[test]
    fn the_casts_the_frontends_build_are_transparent() {
        for (cast, from, to) in [
            (CastOp::PtrToInt, pointer(Type::Felt), Type::U32),
            (CastOp::PtrToInt, pointer(Type::U8), Type::I32),
            (CastOp::IntToPtr, Type::I32, pointer(Type::Felt)),
            (CastOp::IntToPtr, Type::U32, pointer(Type::U64)),
            (CastOp::Bitcast, Type::U32, Type::I32),
            (CastOp::Bitcast, Type::I64, Type::U64),
            (CastOp::Bitcast, Type::U64, Type::I64),
            (CastOp::Bitcast, Type::Felt, Type::I32),
            (CastOp::Bitcast, Type::I32, Type::Felt),
            (CastOp::Bitcast, pointer(Type::U64), pointer(Type::U32)),
        ] {
            verify_cast(cast, from.clone(), to.clone())
                .unwrap_or_else(|err| panic!("{cast:?} of {from} to {to} should verify: {err}"));
        }
    }

    /// An `inttoptr` of a felt would have to check that the felt is a `u32`, so it is not a
    /// transparent cast; no frontend builds one.
    #[test]
    fn an_inttoptr_of_a_felt_is_rejected() {
        let err = verify_cast(CastOp::IntToPtr, Type::Felt, pointer(Type::U32)).unwrap_err();
        let message = format!("{err:?}").split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(message.contains("converted to `u32` with `hir.cast` first"), "{message}");
    }
}

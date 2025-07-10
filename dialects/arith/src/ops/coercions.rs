use alloc::boxed::Box;

use midenc_hir::{
    derive::operation, effects::MemoryEffectOpInterface, matchers::Matcher, traits::*, *,
};

use crate::*;

#[operation(
    dialect = ArithDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Trunc {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyInteger,
}

has_no_effects!(Trunc);

impl InferTypeOpInterface for Trunc {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Trunc {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.operand().as_operand_ref())
        {
            let truncated = match self.ty() {
                Type::I1 => value.as_u64().map(|v| Immediate::I1((v & 0x01u64) == 1)),
                Type::I8 => value.as_i64().map(|v| Immediate::I8(v as i8)),
                Type::U8 => value.as_u64().map(|v| Immediate::U8(v as u8)),
                Type::I16 => value.as_i64().map(|v| Immediate::I16(v as i16)),
                Type::U16 => value.as_u64().map(|v| Immediate::U16(v as u16)),
                Type::I32 => value.as_i64().map(|v| Immediate::I32(v as i32)),
                Type::U32 => value.as_u64().map(|v| Immediate::U32(v as u32)),
                Type::I64 => value.as_i128().map(|v| Immediate::I64(v as i64)),
                Type::U64 => value.as_u128().map(|v| Immediate::U64(v as u64)),
                _ => return FoldResult::Failed,
            };

            if let Some(truncated) = truncated {
                *value = truncated;
                results.push(OpFoldResult::Attribute(value));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            let truncated = match self.ty() {
                Type::I1 => value.as_u64().map(|v| Immediate::I1((v & 0x01u64) == 1)),
                Type::I8 => value.as_i64().map(|v| Immediate::I8(v as i8)),
                Type::U8 => value.as_u64().map(|v| Immediate::U8(v as u8)),
                Type::I16 => value.as_i64().map(|v| Immediate::I16(v as i16)),
                Type::U16 => value.as_u64().map(|v| Immediate::U16(v as u16)),
                Type::I32 => value.as_i64().map(|v| Immediate::I32(v as i32)),
                Type::U32 => value.as_u64().map(|v| Immediate::U32(v as u32)),
                Type::I64 => value.as_i128().map(|v| Immediate::I64(v as i64)),
                Type::U64 => value.as_u128().map(|v| Immediate::U64(v as u64)),
                _ => return FoldResult::Failed,
            };
            if let Some(truncated) = truncated {
                results.push(OpFoldResult::Attribute(Box::new(truncated)));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }
}

#[operation(
    dialect = ArithDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Zext {
    #[operand]
    operand: AnyUnsignedInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyUnsignedInteger,
}

has_no_effects!(Zext);

impl InferTypeOpInterface for Zext {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Zext {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.operand().as_operand_ref())
        {
            let extended = match self.ty() {
                Type::U8 => value.as_u32().and_then(|v| u8::try_from(v).ok()).map(Immediate::U8),
                Type::U16 => value.as_u32().and_then(|v| u16::try_from(v).ok()).map(Immediate::U16),
                Type::U32 => value.as_u32().map(Immediate::U32),
                Type::U64 => value.as_u64().map(Immediate::U64),
                Type::U128 => value.as_u128().map(Immediate::U128),
                _ => return FoldResult::Failed,
            };

            if let Some(extended) = extended {
                *value = extended;
                results.push(OpFoldResult::Attribute(value));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            let extended = match self.ty() {
                Type::U8 => value.as_u32().and_then(|v| u8::try_from(v).ok()).map(Immediate::U8),
                Type::U16 => value.as_u32().and_then(|v| u16::try_from(v).ok()).map(Immediate::U16),
                Type::U32 => value.as_u32().map(Immediate::U32),
                Type::U64 => value.as_u64().map(Immediate::U64),
                Type::U128 => value.as_u128().map(Immediate::U128),
                _ => return FoldResult::Failed,
            };
            if let Some(extended) = extended {
                results.push(OpFoldResult::Attribute(Box::new(extended)));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }
}

#[operation(
    dialect = ArithDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface, Foldable)
)]
pub struct Sext {
    #[operand]
    operand: AnySignedInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnySignedInteger,
}

has_no_effects!(Sext);

impl InferTypeOpInterface for Sext {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty);
        Ok(())
    }
}

impl Foldable for Sext {
    fn fold(&self, results: &mut SmallVec<[OpFoldResult; 1]>) -> FoldResult {
        if let Some(mut value) =
            matchers::foldable_operand_of::<Immediate>().matches(&self.operand().as_operand_ref())
        {
            let extended = match self.ty() {
                Type::I8 => value.as_i32().and_then(|v| i8::try_from(v).ok()).map(Immediate::I8),
                Type::I16 => value.as_i32().and_then(|v| i16::try_from(v).ok()).map(Immediate::I16),
                Type::I32 => value.as_i32().map(Immediate::I32),
                Type::I64 => value.as_i64().map(Immediate::I64),
                Type::I128 => value.as_i128().map(Immediate::I128),
                _ => return FoldResult::Failed,
            };

            if let Some(extended) = extended {
                *value = extended;
                results.push(OpFoldResult::Attribute(value));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }

    fn fold_with(
        &self,
        operands: &[Option<Box<dyn AttributeValue>>],
        results: &mut SmallVec<[OpFoldResult; 1]>,
    ) -> FoldResult {
        if let Some(value) = operands[0].as_deref().and_then(|o| o.downcast_ref::<Immediate>()) {
            let extended = match self.ty() {
                Type::I8 => value.as_i32().and_then(|v| i8::try_from(v).ok()).map(Immediate::I8),
                Type::I16 => value.as_i32().and_then(|v| i16::try_from(v).ok()).map(Immediate::I16),
                Type::I32 => value.as_i32().map(Immediate::I32),
                Type::I64 => value.as_i64().map(Immediate::I64),
                Type::I128 => value.as_i128().map(Immediate::I128),
                _ => return FoldResult::Failed,
            };
            if let Some(extended) = extended {
                results.push(OpFoldResult::Attribute(Box::new(extended)));
                return FoldResult::Ok(());
            }
        }

        FoldResult::Failed
    }
}

/// Join two 64bit integers into one 128 bit integer.
#[operation(
    dialect = ArithDialect,
    traits(BinaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Join {
    #[operand]
    lhs: Int64,
    #[operand]
    rhs: Int64,
    #[result]
    result: SizedInt<128>,
}

has_no_effects!(Join);

impl InferTypeOpInterface for Join {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_mut().set_type(Type::I128);
        Ok(())
    }
}

/// Split a 128bit integer into two 64 bit integers.
#[operation(
    dialect = ArithDialect,
    traits(UnaryOp, SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Split {
    #[operand]
    operand: SizedInt<128>,
    #[result]
    result_high: Int64,
    #[result]
    result_low: Int64,
}

has_no_effects!(Split);

impl InferTypeOpInterface for Split {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        self.result_low_mut().set_type(Type::I64);
        self.result_high_mut().set_type(Type::I64);
        Ok(())
    }
}

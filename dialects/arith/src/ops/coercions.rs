use alloc::{boxed::Box, format};

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

/// Returns true if `ty` is a valid 32-bit limb type for `arith.split`/`arith.join`.
fn is_32bit_limb(ty: &Type) -> bool {
    matches!(ty, Type::Felt | Type::I32 | Type::U32)
}

/// Returns true if `ty` is a valid 64-bit limb type for `arith.split`/`arith.join`.
fn is_64bit_limb(ty: &Type) -> bool {
    matches!(ty, Type::I64 | Type::U64)
}

/// Join limbs into an integer value.
///
/// The limbs are provided in most-significant to least-significant order.
///
/// This operation supports the following combinations:
///
/// - `i64`/`u64` from 2× `felt`/`i32`/`u32`
/// - `i128`/`u128` from 2× `i64`/`u64`
/// - `i128`/`u128` from 4× `felt`/`i32`/`u32`
#[operation(
    dialect = ArithDialect,
    traits(SameTypeOperands),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Join {
    #[operands]
    limbs: AnyInteger,
    #[attr(hidden)]
    ty: Type,
    #[result]
    result: AnyInteger,
}

has_no_effects!(Join);

impl InferTypeOpInterface for Join {
    fn infer_return_types(&mut self, _context: &Context) -> Result<(), Report> {
        let ty = self.ty().clone();
        self.result_mut().set_type(ty.clone());

        let num_limbs = self.limbs().len();
        if !matches!(num_limbs, 2 | 4) {
            return Err(Report::msg(format!(
                "invalid operation arith.join: expected 2 or 4 limbs, but got {num_limbs}"
            )));
        }

        let limb_ty = self.limbs()[0].borrow().as_value_ref().borrow().ty().clone();
        let is_limb_ty_32bit = is_32bit_limb(&limb_ty);
        let is_limb_ty_64bit = is_64bit_limb(&limb_ty);
        let is_valid = matches!(
            (&ty, num_limbs, is_limb_ty_32bit, is_limb_ty_64bit),
            (&Type::I64 | &Type::U64, 2, true, _)
                | (&Type::I128 | &Type::U128, 2, _, true)
                | (&Type::I128 | &Type::U128, 4, true, _)
        );
        if !is_valid {
            return Err(Report::msg(format!(
                "invalid operation arith.join: cannot join {num_limbs} limb(s) of type \
                 '{limb_ty}' into '{ty}'"
            )));
        }

        Ok(())
    }
}

/// Split an integer into one or more limbs.
///
/// The limbs are returned in most-significant to least-significant order.
///
/// This operation supports the following combinations:
///
/// - `i64`/`u64` into 2× `felt`/`i32`/`u32`
/// - `i128`/`u128` into 2× `i64`/`u64`
/// - `i128`/`u128` into 4× `felt`/`i32`/`u32`
#[operation(
    dialect = ArithDialect,
    traits(UnaryOp),
    implements(InferTypeOpInterface, MemoryEffectOpInterface)
)]
pub struct Split {
    #[operand]
    operand: AnyInteger,
    #[attr(hidden)]
    limb_ty: Type,
    #[results]
    limbs: AnyInteger,
}

has_no_effects!(Split);

impl InferTypeOpInterface for Split {
    fn infer_return_types(&mut self, context: &Context) -> Result<(), Report> {
        let operand_ty = self.operand().as_value_ref().borrow().ty().clone();
        let limb_ty = self.limb_ty().clone();

        let num_limbs = match operand_ty {
            Type::I64 | Type::U64 if is_32bit_limb(&limb_ty) => 2,
            Type::I128 | Type::U128 if is_64bit_limb(&limb_ty) => 2,
            Type::I128 | Type::U128 if is_32bit_limb(&limb_ty) => 4,
            _ => {
                return Err(Report::msg(format!(
                    "invalid operation arith.split: cannot split '{operand_ty}' into limb type \
                     '{limb_ty}'"
                )));
            }
        };

        // We infer the number of limbs from (operand_ty, limb_ty), and once created, the result
        // count must remain stable.
        //
        // When building `arith.split`, the op initially has no results, and we create them here.
        // When validating an existing op, we ensure the result count matches what we would infer.
        if !self.op.results.is_empty() && self.op.results.len() != num_limbs {
            return Err(Report::msg(format!(
                "invalid operation arith.split: expected {num_limbs} result(s) for '{operand_ty}' \
                 split into '{limb_ty}', but got {}",
                self.op.results.len()
            )));
        }

        if self.op.results.is_empty() {
            let span = self.span();
            let owner = self.as_operation_ref();
            for i in 0..num_limbs {
                let value = context.make_result(span, limb_ty.clone(), owner, i as u8);
                self.op.results.push(value);
            }
        } else {
            for result in self.op.results.iter_mut() {
                result.borrow_mut().set_type(limb_ty.clone());
            }
        }

        Ok(())
    }
}

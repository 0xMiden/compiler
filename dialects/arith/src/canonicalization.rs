use alloc::rc::Rc;

use midenc_hir::{
    interner::Symbol,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// Canonicalizes 32-bit rotations of `i64`/`u64` values into a swap of the 32-bit limbs.
///
/// This is used to preserve the "extra" bits that may be present when `i64` values are actually
/// being used to operate over two packed `f32` values in memory.
pub(crate) struct CanonicalizeI64RotateBy32ToSwap {
    info: PatternInfo,
}

impl CanonicalizeI64RotateBy32ToSwap {
    /// Create a canonicalization pattern for `op`.
    pub fn for_op(context: Rc<Context>, op: OperationName) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "canonicalize-i64-rotate-by-32-to-swap",
                PatternKind::Operation(op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for CanonicalizeI64RotateBy32ToSwap {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for CanonicalizeI64RotateBy32ToSwap {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let hir = Symbol::intern("hir");
        let arith = Symbol::intern("arith");
        let cast = Symbol::intern("cast");
        let bitcast = Symbol::intern("bitcast");
        let trunc = Symbol::intern("trunc");
        let sext = Symbol::intern("sext");
        let zext = Symbol::intern("zext");
        let max_depth = 4usize;

        // Try to recover a `u32` constant value from `value`, allowing a small set of passthrough
        // casts/conversions, e.g. `hir.cast(arith.constant 32 : i64) : u32`.
        let constant_u32 = |value: ValueRef| -> Option<u32> {
            let mut current = value;
            for _ in 0..max_depth {
                let defining_op = current.borrow().get_defining_op()?;
                let op = defining_op.borrow();
                if let Some(constant) = op.downcast_ref::<Constant>() {
                    let imm = constant.value();
                    return imm
                        .as_u32()
                        .or_else(|| imm.as_i32().and_then(|v| u32::try_from(v).ok()))
                        .or_else(|| imm.as_u64().and_then(|v| u32::try_from(v).ok()));
                }

                let name = op.name();
                let dialect = name.dialect();
                let opcode = name.name();
                let is_passthrough = (dialect == hir && (opcode == cast || opcode == bitcast))
                    || (dialect == arith && (opcode == trunc || opcode == sext || opcode == zext));
                if !is_passthrough {
                    return None;
                }

                let mut operands = op.operands().iter();
                let operand = operands.next()?;
                current = operand.borrow().as_value_ref();
            }

            None
        };

        let (span, lhs, lhs_ty, is_rotate_by_32) = {
            let op = operation.borrow();
            let span = op.span();
            let Some((lhs, shift)) = op
                .downcast_ref::<Rotl>()
                .map(|rotl| (rotl.lhs().as_value_ref(), rotl.shift().as_operand_ref()))
                .or_else(|| {
                    op.downcast_ref::<Rotr>()
                        .map(|rotr| (rotr.lhs().as_value_ref(), rotr.shift().as_operand_ref()))
                })
            else {
                return Ok(false);
            };

            let lhs_ty = lhs.borrow().ty().clone();
            if !matches!(lhs_ty, Type::I64 | Type::U64) {
                return Ok(false);
            }

            let is_rotate_by_32 = constant_u32(shift.borrow().as_value_ref()) == Some(32);

            (span, lhs, lhs_ty, is_rotate_by_32)
        };

        if !is_rotate_by_32 {
            return Ok(false);
        }

        rewriter.set_insertion_point_before(operation);

        // Split the `i64` into (felt, felt) limbs, swap them, then join back to the original type.
        //
        // Using felts (instead of 32-bit integer types) ensures the underlying values are not
        // range-checked/normalized, which is required to preserve any extra bits.
        let split = {
            let op_builder = rewriter.create::<Split, _>(span);
            op_builder(lhs, Type::Felt)?
        };
        let (hi, lo) = {
            let split = split.borrow();
            (split.result_high().as_value_ref(), split.result_low().as_value_ref())
        };
        let joined = {
            let op_builder = rewriter.create::<Join, (ValueRef, ValueRef, Type)>(span);
            let join = op_builder(lo, hi, lhs_ty)?;
            join.borrow().result().as_value_ref()
        };

        rewriter.replace_op_with_values(operation, &[Some(joined)]);
        Ok(true)
    }
}

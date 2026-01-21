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

        let mut guard = InsertionGuard::new(rewriter);
        guard.set_insertion_point_before(operation);

        // Split the `i64` into (felt, felt) limbs, swap them, then join back to the original type.
        //
        // Using felts (instead of 32-bit integer types) ensures the underlying values are not
        // range-checked/normalized, which is required to preserve any extra bits.
        let split = {
            let op_builder = guard.create::<Split, _>(span);
            op_builder(lhs, Type::Felt)?
        };
        let (hi, lo) = {
            let split = split.borrow();
            (split.result_high().as_value_ref(), split.result_low().as_value_ref())
        };
        let joined = {
            let op_builder = guard.create::<Join, (ValueRef, ValueRef, Type)>(span);
            let join = op_builder(lo, hi, lhs_ty)?;
            join.borrow().result().as_value_ref()
        };

        guard.replace_op_with_values(operation, &[Some(joined)]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{
        AbiParam, BuilderExt, Context, Ident, OpBuilder, Report, Signature, SourceSpan, Type,
        dialects::builtin::{BuiltinOpBuilder, Function, FunctionBuilder},
        patterns::{
            FrozenRewritePatternSet, GreedyRewriteConfig, RegionSimplificationLevel, RewritePatternSet,
            apply_patterns_and_fold_greedily,
        },
        traits::Canonicalizable,
    };

    use crate::{ArithDialect, ArithOpBuilder, Join, Rotl, Rotr, Split};

    fn build_rotate_by_32(
        context: Rc<Context>,
        ty: Type,
        is_rotr: bool,
    ) -> Result<midenc_hir::OperationRef, Report> {
        let _arith = context.get_or_register_dialect::<ArithDialect>();

        let span = SourceSpan::default();
        let mut builder = OpBuilder::new(context);

        let function = {
            let builder = builder.create::<Function, (_, _)>(span);
            let name = Ident::new("test".into(), span);
            let signature = Signature::new([AbiParam::new(ty.clone())], [AbiParam::new(ty.clone())]);
            builder(name, signature)?
        };

        {
            let mut builder = FunctionBuilder::new(function, &mut builder);
            let input = builder.current_block().borrow().arguments()[0].upcast();
            let shift = builder.u32(32, span);
            let rotated = if is_rotr {
                builder.rotr(input, shift, span)?
            } else {
                builder.rotl(input, shift, span)?
            };
            builder.ret(Some(rotated), span)?;
        }

        Ok(function.as_operation_ref())
    }

    fn apply_rotate_canonicalization(
        context: Rc<Context>,
        function: midenc_hir::OperationRef,
    ) -> bool {
        let mut patterns = RewritePatternSet::new(context.clone());
        Rotl::get_canonicalization_patterns(&mut patterns, context.clone());
        Rotr::get_canonicalization_patterns(&mut patterns, context);
        let patterns = Rc::new(FrozenRewritePatternSet::new(patterns));

        let mut config = GreedyRewriteConfig::default();
        config.with_region_simplification_level(RegionSimplificationLevel::None);

        match apply_patterns_and_fold_greedily(function, patterns, config) {
            Ok(changed) => changed,
            Err(changed) => panic!("canonicalization failed (changed={changed})"),
        }
    }

    fn assert_rotate_by_32_rewritten(function: midenc_hir::OperationRef, ty: Type) {
        let body = {
            let function = function.borrow();
            let function = function.downcast_ref::<Function>().expect("expected builtin.function");
            function.body().as_region_ref()
        };
        let entry = body
            .borrow()
            .entry_block_ref()
            .expect("expected function body to have an entry block");

        let mut rotl = false;
        let mut rotr = false;
        let mut split = None;
        let mut join = None;

        for op in entry.borrow().body() {
            let op = op.as_operation_ref();
            let operation = op.borrow();

            rotl |= operation.downcast_ref::<Rotl>().is_some();
            rotr |= operation.downcast_ref::<Rotr>().is_some();

            if operation.downcast_ref::<Split>().is_some() {
                assert!(split.replace(op).is_none(), "expected a single arith.split");
            }
            if operation.downcast_ref::<Join>().is_some() {
                assert!(join.replace(op).is_none(), "expected a single arith.join");
            }
        }

        assert!(!rotl, "expected arith.rotl to be eliminated");
        assert!(!rotr, "expected arith.rotr to be eliminated");

        let split = split.expect("expected arith.split");
        let join = join.expect("expected arith.join");

        let (hi, lo) = {
            let split = split.borrow();
            let split = split.downcast_ref::<Split>().unwrap();
            assert_eq!(
                split.limb_ty(),
                &Type::Felt,
                "expected split to use `felt` limbs"
            );
            (split.result_high().as_value_ref(), split.result_low().as_value_ref())
        };

        let (high, low) = {
            let join = join.borrow();
            let join = join.downcast_ref::<Join>().unwrap();
            assert_eq!(
                join.ty(),
                &ty,
                "expected join to reconstruct the original rotate type"
            );
            (join.high_limb().as_value_ref(), join.low_limb().as_value_ref())
        };

        assert_eq!(high, lo, "expected join high limb to use split low limb");
        assert_eq!(low, hi, "expected join low limb to use split high limb");
    }

    #[test]
    fn canonicalize_rotl_u64_by_32_to_swap() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = build_rotate_by_32(context.clone(), Type::U64, false)?;

        assert_eq!(apply_rotate_canonicalization(context.clone(), function), true);
        assert_rotate_by_32_rewritten(function, Type::U64);

        Ok(())
    }

    #[test]
    fn canonicalize_rotr_i64_by_32_to_swap() -> Result<(), Report> {
        let context = Rc::new(Context::default());
        let function = build_rotate_by_32(context.clone(), Type::I64, true)?;

        assert_eq!(apply_rotate_canonicalization(context.clone(), function), true);
        assert_rotate_by_32_rewritten(function, Type::I64);

        Ok(())
    }
}

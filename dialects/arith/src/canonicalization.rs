//! Canonicalization patterns for the `arith` dialect.
//!
//! The patterns are:
//!
//! * `canonicalize-i64-rotate-by-32-to-swap`: 32-bit rotates of `i64`/`u64` become a swap of the
//!   32-bit limbs, preserving the full 64-bit payload when the values represent felts.
//! * `simplify-bool-compare`: `eq`/`neq` of a widened boolean against 0 or 1 becomes the boolean
//!   or its negation.
//! * `simplify-not-compare`: `not` of a single-use `eq`/`neq` becomes the complementary
//!   comparison.
//! * `simplify-trunc-to-bool`: `trunc` to `i1` of a widened boolean becomes the boolean.

use alloc::rc::Rc;

use midenc_hir::{
    interner::Symbol,
    matchers::Matcher,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    traits::TransparentCast,
    *,
};

use crate::*;

/// Canonicalizes 32-bit rotations of `i64`/`u64` values into a swap of the 32-bit limbs.
///
/// This is used to preserve the "extra" bits that may be present when `i64` values are actually
/// being used to operate over two felt values in memory.
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
            let [hi, lo] = split.limbs().as_slice() else {
                unreachable!("expected arith.split to produce 2 limbs for i64/u64");
            };
            (hi.borrow().as_value_ref(), lo.borrow().as_value_ref())
        };
        let joined = {
            let op_builder = guard.create::<Join, ([ValueRef; 2], Type)>(span);
            let join = op_builder([lo, hi], lhs_ty)?;
            join.borrow().result().as_value_ref()
        };

        guard.replace_op_with_values(operation, &[Some(joined)]);
        Ok(true)
    }
}

/// How many widening/transparent-cast operations [widened_bool] looks through at most.
const MAX_WIDENING_DEPTH: usize = 4;

/// Returns the `i1` value that `value` is a widening of.
///
/// Looks through up to [MAX_WIDENING_DEPTH] `arith.zext` and transparent casts between integer
/// types, which preserve the integer value 0/1 of a widened boolean. Returns `value` itself when it
/// is already an `i1`. Sign extension is deliberately not looked through (`true` widens to -1).
fn widened_bool(value: ValueRef) -> Option<ValueRef> {
    let mut current = value;
    // One more check than steps, so the value reached by the last step is still inspected.
    for _ in 0..=MAX_WIDENING_DEPTH {
        if current.borrow().ty() == &Type::I1 {
            return Some(current);
        }
        let defining_op = current.borrow().get_defining_op()?;
        let op = defining_op.borrow();
        let is_widening =
            op.downcast_ref::<Zext>().is_some() || op.implements::<dyn TransparentCast>();
        if !is_widening || op.num_operands() != 1 {
            return None;
        }
        let operand = op.operands()[0].borrow().as_value_ref();
        if !operand.borrow().ty().is_integer() {
            return None;
        }
        current = operand;
    }
    None
}

/// Returns the boolean a constant 0 or 1 integer `value` stands for, or `None` for anything else.
fn constant_bit(value: ValueRef) -> Option<bool> {
    let defining_op = value.borrow().get_defining_op()?;
    let imm = matchers::constant_of::<Immediate>().matches(&*defining_op.borrow())?;
    match imm.as_u64()? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// Returns true if `value` has exactly one use.
fn has_single_use(value: ValueRef) -> bool {
    let value = value.borrow();
    let mut uses = value.iter_uses();
    uses.next().is_some() && uses.next().is_none()
}

/// Simplifies `arith.eq`/`arith.neq` of a widened boolean against a constant 0 or 1 to the
/// boolean itself or its negation.
///
/// Frontends commonly widen booleans to an integer and test them against 0 again before a branch
/// or select, so these comparisons are common, and each one costs several VM ops while the boolean
/// is already at hand:
///
/// * `neq(x, 0)` and `eq(x, 1)` become `b`
/// * `eq(x, 0)` and `neq(x, 1)` become `not b`
///
/// where `b` is the `i1` that `x` widens (see [widened_bool]).
pub(crate) struct SimplifyBoolCompare {
    info: PatternInfo,
}

impl SimplifyBoolCompare {
    /// Create the pattern for `op`, which must be `arith.eq` or `arith.neq`.
    pub fn for_op(context: Rc<Context>, op: OperationName) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "simplify-bool-compare",
                PatternKind::Operation(op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyBoolCompare {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyBoolCompare {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let (span, is_eq, lhs, rhs) = {
            let op = operation.borrow();
            let span = op.span();
            let (is_eq, lhs, rhs) = if let Some(eq) = op.downcast_ref::<Eq>() {
                (true, eq.lhs().as_value_ref(), eq.rhs().as_value_ref())
            } else if let Some(neq) = op.downcast_ref::<Neq>() {
                (false, neq.lhs().as_value_ref(), neq.rhs().as_value_ref())
            } else {
                return Ok(false);
            };
            (span, is_eq, lhs, rhs)
        };

        let (bool_value, bit) = if let Some(bit) = constant_bit(rhs) {
            (widened_bool(lhs), bit)
        } else if let Some(bit) = constant_bit(lhs) {
            (widened_bool(rhs), bit)
        } else {
            return Ok(false);
        };
        let Some(bool_value) = bool_value else {
            return Ok(false);
        };

        // `eq(b, true)`/`neq(b, false)` is `b`; the other two combinations negate it.
        let replacement = if is_eq == bit {
            bool_value
        } else {
            rewriter.set_insertion_point_before(operation);
            rewriter.not(bool_value, span)?
        };
        rewriter.replace_op_with_values(operation, &[Some(replacement)]);
        Ok(true)
    }
}

/// Simplifies `arith.not` of a single-use `arith.eq`/`arith.neq` into the complementary
/// comparison, so the negation disappears from the generated code.
pub(crate) struct SimplifyNotCompare {
    info: PatternInfo,
}

impl SimplifyNotCompare {
    /// Creates the pattern, matching `arith.not`.
    pub fn new(context: Rc<Context>) -> Self {
        let not_op = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Not>();
        Self {
            info: PatternInfo::new(
                context,
                "simplify-not-compare",
                PatternKind::Operation(not_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyNotCompare {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyNotCompare {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let (span, operand) = {
            let op = operation.borrow();
            let Some(not) = op.downcast_ref::<Not>() else {
                return Ok(false);
            };
            (op.span(), not.operand().as_value_ref())
        };
        if !has_single_use(operand) {
            return Ok(false);
        }
        let Some(compare) = operand.borrow().get_defining_op() else {
            return Ok(false);
        };
        let (is_eq, lhs, rhs) = {
            let compare = compare.borrow();
            if let Some(eq) = compare.downcast_ref::<Eq>() {
                (true, eq.lhs().as_value_ref(), eq.rhs().as_value_ref())
            } else if let Some(neq) = compare.downcast_ref::<Neq>() {
                (false, neq.lhs().as_value_ref(), neq.rhs().as_value_ref())
            } else {
                return Ok(false);
            }
        };

        // The complementary comparison takes the place of the original one, not of the `not`:
        // the `not` may sit in a nested region (a loop body), and rebuilding the comparison there
        // would evaluate it per iteration and keep its operands live across the region.
        rewriter.set_insertion_point_after(compare);
        let replacement = if is_eq {
            rewriter.neq(lhs, rhs, span)?
        } else {
            rewriter.eq(lhs, rhs, span)?
        };
        rewriter.replace_op_with_values(operation, &[Some(replacement)]);
        Ok(true)
    }
}

/// Simplifies `arith.trunc` to `i1` of a widened boolean into the boolean itself.
pub(crate) struct SimplifyTruncToBool {
    info: PatternInfo,
}

impl SimplifyTruncToBool {
    /// Creates the pattern, matching `arith.trunc`.
    pub fn new(context: Rc<Context>) -> Self {
        let trunc_op = context
            .get_or_register_dialect::<ArithDialect>()
            .expect_registered_name::<Trunc>();
        Self {
            info: PatternInfo::new(
                context,
                "simplify-trunc-to-bool",
                PatternKind::Operation(trunc_op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for SimplifyTruncToBool {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for SimplifyTruncToBool {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let operand = {
            let op = operation.borrow();
            let Some(trunc) = op.downcast_ref::<Trunc>() else {
                return Ok(false);
            };
            if trunc.result().ty() != &Type::I1 {
                return Ok(false);
            }
            trunc.operand().as_value_ref()
        };
        let Some(bool_value) = widened_bool(operand) else {
            return Ok(false);
        };
        rewriter.replace_op_with_values(operation, &[Some(bool_value)]);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use alloc::rc::Rc;

    use midenc_hir::{
        Op, Report, SourceSpan, Type,
        dialects::builtin::{BuiltinOpBuilder, Function},
        patterns::{
            FrozenRewritePatternSet, GreedyRewriteConfig, RegionSimplificationLevel,
            RewritePatternSet, apply_patterns_and_fold_greedily,
        },
        testing::Test,
        traits::Canonicalizable,
    };

    use crate::{ArithOpBuilder, Join, Rotl, Rotr, Split};

    fn build_rotate_by_32(test: &mut Test, ty: Type, is_rotr: bool) -> Result<(), Report> {
        let span = SourceSpan::default();

        test.with_function("test", core::slice::from_ref(&ty), core::slice::from_ref(&ty));

        {
            let mut builder = test.function_builder();
            let input = builder.current_block().borrow().arguments()[0].upcast();
            let shift = builder.u32(32, span);
            let rotated = if is_rotr {
                builder.rotr(input, shift, span)?
            } else {
                builder.rotl(input, shift, span)?
            };
            builder.ret(Some(rotated), span)?;
        }

        Ok(())
    }

    fn apply_rotate_canonicalization(test: &Test) -> bool {
        let context = test.context_rc();
        let mut patterns = RewritePatternSet::new(context.clone());
        Rotl::get_canonicalization_patterns(&mut patterns, context.clone());
        Rotr::get_canonicalization_patterns(&mut patterns, context);
        let patterns = Rc::new(FrozenRewritePatternSet::new(patterns));

        let mut config = GreedyRewriteConfig::default();
        config.with_region_simplification_level(RegionSimplificationLevel::None);

        let function = test.function().as_operation_ref();
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
            assert_eq!(&*split.get_limb_ty(), &Type::Felt, "expected split to use `felt` limbs");
            let [hi, lo] = split.limbs().as_slice() else {
                panic!("expected arith.split to produce 2 limbs for i64/u64");
            };
            (hi.borrow().as_value_ref(), lo.borrow().as_value_ref())
        };

        let (high, low) = {
            let join = join.borrow();
            let join = join.downcast_ref::<Join>().unwrap();
            assert_eq!(
                &*join.get_ty(),
                &ty,
                "expected join to reconstruct the original rotate type"
            );
            let [high, low] = join.limbs().as_slice() else {
                panic!("expected arith.join to consume 2 limbs for i64/u64");
            };
            (high.borrow().as_value_ref(), low.borrow().as_value_ref())
        };

        assert_eq!(high, lo, "expected join high limb to use split low limb");
        assert_eq!(low, hi, "expected join low limb to use split high limb");
    }

    #[test]
    fn canonicalize_rotl_u64_by_32_to_swap() -> Result<(), Report> {
        let mut test = Test::named("canonicalize_rotl_u64_by_32_to_swap");
        build_rotate_by_32(&mut test, Type::U64, false)?;

        assert!(apply_rotate_canonicalization(&test));
        assert_rotate_by_32_rewritten(test.function().as_operation_ref(), Type::U64);

        Ok(())
    }

    #[test]
    fn canonicalize_rotr_i64_by_32_to_swap() -> Result<(), Report> {
        let mut test = Test::named("canonicalize_rotr_i64_by_32_to_swap");
        build_rotate_by_32(&mut test, Type::I64, true)?;

        assert!(apply_rotate_canonicalization(&test));
        assert_rotate_by_32_rewritten(test.function().as_operation_ref(), Type::I64);

        Ok(())
    }

    #[test]
    fn inline_metadata_survives_rotate_rewrite() -> Result<(), Report> {
        check_inline_metadata_survives_rotate_rewrite(false)
    }

    #[test]
    fn standalone_pattern_application_inherits_inline_metadata() -> Result<(), Report> {
        check_inline_metadata_survives_rotate_rewrite(true)
    }

    fn check_inline_metadata_survives_rotate_rewrite(standalone: bool) -> Result<(), Report> {
        use midenc_hir::dialects::debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr, InlineCallFrame,
        };
        let mut test = Test::named("inline_metadata_survives_rotate_rewrite");
        build_rotate_by_32(&mut test, Type::U64, false)?;
        let function = test.function().as_operation_ref();
        let mut rotate = None;
        function.borrow().prewalk_all(|op| {
            if op.is::<Rotl>() {
                rotate = Some(op.as_operation_ref());
            }
        });
        let attr = test
            .context_rc()
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![
                InlineCallFrame {
                    name: "inline_rotate".into(),
                    linkage_name: None,
                    file: "source.rs".into(),
                    line: 1,
                    column: 1,
                    call_file: "source.rs".into(),
                    call_line: 2,
                    call_column: 1,
                }
            ]))
            .as_attribute_ref();
        let mut rotate = rotate.unwrap();
        rotate.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, attr);
        if standalone {
            use midenc_hir::patterns::{NoopRewriterListener, PatternApplicator, RewriterImpl};
            let context = test.context_rc();
            let mut patterns = RewritePatternSet::new(context.clone());
            Rotl::get_canonicalization_patterns(&mut patterns, context.clone());
            let mut applicator =
                PatternApplicator::new(Rc::new(FrozenRewritePatternSet::new(patterns)));
            applicator.apply_cost_model(|pattern| *pattern.benefit());
            let mut rewriter = RewriterImpl::<NoopRewriterListener>::new(context);
            assert!(
                applicator
                    .match_and_rewrite(rotate, &mut rewriter, |_| true, |_| {}, |_| Ok(()))
                    .is_ok()
            );
        } else {
            assert!(apply_rotate_canonicalization(&test));
        }
        let mut marked_replacements = 0;
        function.borrow().prewalk_all(|op| {
            if op.is::<Split>() || op.is::<Join>() {
                assert_eq!(op.get_attribute(INLINE_CALL_CHAIN_ATTR_NAME), Some(attr));
                marked_replacements += 1;
            }
        });
        assert_eq!(marked_replacements, 2, "both replacement ops must retain their inline frame");
        Ok(())
    }

    #[test]
    fn inline_metadata_survives_constant_folding() -> Result<(), Report> {
        use midenc_hir::dialects::debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr,
        };
        let mut test = Test::new("inline_metadata_survives_constant_folding", &[], &[Type::U32]);
        let chain = test
            .context_rc()
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::default())
            .as_attribute_ref();
        {
            let mut builder = test.function_builder();
            let input = builder.u64(33, SourceSpan::UNKNOWN);
            let truncated = builder.trunc(input, Type::U32, SourceSpan::UNKNOWN)?;
            truncated
                .borrow()
                .get_defining_op()
                .unwrap()
                .borrow_mut()
                .set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
            builder.ret(Some(truncated), SourceSpan::UNKNOWN)?;
        }
        assert!(apply_rotate_canonicalization(&test));
        let mut returns = 0;
        test.function().borrow().as_operation().prewalk_all(|op| {
            if op.is::<midenc_hir::dialects::builtin::Ret>() {
                let value = op.operands().all()[0].borrow().as_value_ref();
                let constant = value.borrow().get_defining_op().unwrap();
                assert!(constant.borrow().implements::<dyn midenc_hir::traits::ConstantLike>());
                assert_eq!(
                    constant.borrow().get_attribute(INLINE_CALL_CHAIN_ATTR_NAME),
                    Some(chain)
                );
                returns += 1;
            }
        });
        assert_eq!(returns, 1);
        Ok(())
    }
}

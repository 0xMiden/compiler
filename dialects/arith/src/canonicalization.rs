//! Canonicalization patterns for the `arith` dialect.
//!
//! Includes rewrites of 32-bit rotates of `i64`/`u64` values and of 32-bit logical right shifts
//! of `u64` values into limb moves. These preserve the full payload of each 32-bit limb when a
//! 64-bit value actually carries two felt values copied through memory.

use alloc::rc::Rc;

use midenc_hir::{
    interner::Symbol,
    patterns::{Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern},
    *,
};

use crate::*;

/// The maximum number of defining ops [constant_u32] looks through.
const MAX_CONSTANT_DEPTH: usize = 4;

/// Recovers the `u32` constant that `value` evaluates to, if it can be determined locally.
///
/// Looks through a small set of passthrough casts/conversions, e.g.
/// `hir.cast(arith.constant 32 : i64) : u32`, and evaluates `arith.band` of two recoverable
/// constants. Returns `None` for anything else, or when the definition chain is deeper than
/// [MAX_CONSTANT_DEPTH].
fn constant_u32(value: ValueRef) -> Option<u32> {
    constant_u32_bounded(value, MAX_CONSTANT_DEPTH)
}

fn constant_u32_bounded(value: ValueRef, depth: usize) -> Option<u32> {
    if depth == 0 {
        return None;
    }

    let defining_op = value.borrow().get_defining_op()?;
    let op = defining_op.borrow();
    if let Some(constant) = op.downcast_ref::<Constant>() {
        let imm = constant.value();
        return imm
            .as_u32()
            .or_else(|| imm.as_i32().and_then(|v| u32::try_from(v).ok()))
            .or_else(|| imm.as_u64().and_then(|v| u32::try_from(v).ok()));
    }

    // Shift and rotate counts are masked to the bit width by frontends (e.g. Wasm's
    // `count & 63`), so a constant count is usually only visible through the mask.
    if let Some(band) = op.downcast_ref::<Band>() {
        let lhs = constant_u32_bounded(band.lhs().as_value_ref(), depth - 1)?;
        let rhs = constant_u32_bounded(band.rhs().as_value_ref(), depth - 1)?;
        return Some(lhs & rhs);
    }

    // The `hir` dialect ops are matched by name, since `arith` cannot depend on that dialect.
    let name = op.name();
    let dialect = name.dialect();
    let opcode = name.name();
    let is_passthrough = (dialect == Symbol::intern("hir")
        && (opcode == Symbol::intern("cast") || opcode == Symbol::intern("bitcast")))
        || (dialect == Symbol::intern("arith")
            && (opcode == Symbol::intern("trunc")
                || opcode == Symbol::intern("sext")
                || opcode == Symbol::intern("zext")));
    if !is_passthrough {
        return None;
    }

    let operand = op.operands().iter().next()?.borrow().as_value_ref();
    constant_u32_bounded(operand, depth - 1)
}

/// How [replace_with_felt_limbs] recombines the limbs of a 64-bit value.
#[derive(Copy, Clone)]
enum LimbMove {
    /// Swap the high and low limbs (a rotate by 32).
    Swap,
    /// Move the high limb into the low limb and zero the high limb (a logical shift right by 32).
    HighToLow,
}

/// Replaces `operation` with a split of `value` into felt limbs, recombined into `ty` per
/// `limb_move`.
fn replace_with_felt_limbs(
    operation: OperationRef,
    rewriter: &mut dyn Rewriter,
    span: SourceSpan,
    value: ValueRef,
    ty: Type,
    limb_move: LimbMove,
) -> Result<(), Report> {
    let mut guard = InsertionGuard::new(rewriter);
    guard.set_insertion_point_before(operation);

    // Felt limbs (instead of 32-bit integer types) ensure the limbs are not range-checked or
    // normalized, which is required to preserve a felt payload larger than 32 bits.
    let split = {
        let op_builder = guard.create::<Split, _>(span);
        op_builder(value, Type::Felt)?
    };
    let (hi, lo) = {
        let split = split.borrow();
        let [hi, lo] = split.limbs().as_slice() else {
            unreachable!("expected arith.split to produce 2 limbs for i64/u64");
        };
        (hi.borrow().as_value_ref(), lo.borrow().as_value_ref())
    };
    let limbs = match limb_move {
        LimbMove::Swap => [lo, hi],
        LimbMove::HighToLow => {
            let op_builder = guard.create::<Constant, _>(span);
            let zero = op_builder(Immediate::Felt(Felt::ZERO))?;
            [zero.borrow().result().as_value_ref(), hi]
        }
    };
    let joined = {
        let op_builder = guard.create::<Join, ([ValueRef; 2], Type)>(span);
        let join = op_builder(limbs, ty)?;
        join.borrow().result().as_value_ref()
    };

    guard.replace_op_with_values(operation, &[Some(joined)]);
    Ok(())
}

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

        replace_with_felt_limbs(operation, rewriter, span, lhs, lhs_ty, LimbMove::Swap)?;
        Ok(true)
    }
}

/// Canonicalizes 32-bit logical right shifts of `u64` values into a move of the high limb into
/// the low limb, with a zero high limb.
///
/// Like [CanonicalizeI64RotateBy32ToSwap], this preserves the payload of a limb that actually
/// holds a felt value copied through memory.
pub(crate) struct CanonicalizeU64ShrBy32ToLimbMove {
    info: PatternInfo,
}

impl CanonicalizeU64ShrBy32ToLimbMove {
    /// Create a canonicalization pattern for `op`.
    pub fn for_op(context: Rc<Context>, op: OperationName) -> Self {
        Self {
            info: PatternInfo::new(
                context,
                "canonicalize-u64-shr-by-32-to-limb-move",
                PatternKind::Operation(op),
                PatternBenefit::MAX,
            ),
        }
    }
}

impl Pattern for CanonicalizeU64ShrBy32ToLimbMove {
    fn info(&self) -> &PatternInfo {
        &self.info
    }
}

impl RewritePattern for CanonicalizeU64ShrBy32ToLimbMove {
    fn match_and_rewrite(
        &self,
        operation: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        let (span, lhs) = {
            let op = operation.borrow();
            let Some(shr) = op.downcast_ref::<Shr>() else {
                return Ok(false);
            };
            let lhs = shr.lhs().as_value_ref();
            // `I64` shifts are arithmetic (sign-extending), so only `U64` qualifies.
            if *lhs.borrow().ty() != Type::U64
                || constant_u32(shr.shift().as_value_ref()) != Some(32)
            {
                return Ok(false);
            }
            (op.span(), lhs)
        };

        replace_with_felt_limbs(operation, rewriter, span, lhs, Type::U64, LimbMove::HighToLow)?;
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

    use crate::{ArithOpBuilder, Constant, Join, Rotl, Rotr, Shr, Split};

    /// Builds `fn test(x: ty) -> ty { shr(x, shift) }`.
    fn build_shr(test: &mut Test, ty: Type, shift: u32) -> Result<(), Report> {
        let span = SourceSpan::default();

        test.with_function("test", core::slice::from_ref(&ty), core::slice::from_ref(&ty));

        {
            let mut builder = test.function_builder();
            let input = builder.current_block().borrow().arguments()[0].upcast();
            let shift = builder.u32(shift, span);
            let shifted = builder.shr(input, shift, span)?;
            builder.ret(Some(shifted), span)?;
        }

        Ok(())
    }

    fn apply_shr_canonicalization(test: &Test) -> bool {
        let context = test.context_rc();
        let mut patterns = RewritePatternSet::new(context.clone());
        Shr::get_canonicalization_patterns(&mut patterns, context);
        let patterns = Rc::new(FrozenRewritePatternSet::new(patterns));

        let mut config = GreedyRewriteConfig::default();
        config.with_region_simplification_level(RegionSimplificationLevel::None);

        let function = test.function().as_operation_ref();
        match apply_patterns_and_fold_greedily(function, patterns, config) {
            Ok(changed) => changed,
            Err(changed) => panic!("canonicalization failed (changed={changed})"),
        }
    }

    fn contains_shr(test: &Test) -> bool {
        let mut found = false;
        test.function().borrow().as_operation().prewalk_all(|op| {
            found |= op.is::<Shr>();
        });
        found
    }

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
    fn canonicalize_rotl_u64_by_masked_32_to_swap() -> Result<(), Report> {
        let span = SourceSpan::default();
        let mut test = Test::named("canonicalize_rotl_u64_by_masked_32_to_swap");
        test.with_function("test", &[Type::U64], &[Type::U64]);
        {
            // The count shape the Wasm frontend emits: `band(trunc(32 : i64), 63 : u32)`.
            let mut builder = test.function_builder();
            let input = builder.current_block().borrow().arguments()[0].upcast();
            let count = builder.i64(32, span);
            let count = builder.trunc(count, Type::U32, span)?;
            let mask = builder.u32(63, span);
            let count = builder.band(count, mask, span)?;
            let rotated = builder.rotl(input, count, span)?;
            builder.ret(Some(rotated), span)?;
        }

        assert!(apply_rotate_canonicalization(&test));
        assert_rotate_by_32_rewritten(test.function().as_operation_ref(), Type::U64);

        Ok(())
    }

    #[test]
    fn canonicalize_shr_u64_by_32_to_limb_move() -> Result<(), Report> {
        let mut test = Test::named("canonicalize_shr_u64_by_32_to_limb_move");
        build_shr(&mut test, Type::U64, 32)?;

        assert!(apply_shr_canonicalization(&test));
        assert!(!contains_shr(&test), "expected arith.shr to be eliminated");

        let mut split_hi = None;
        let mut join_limbs = None;
        test.function().borrow().as_operation().prewalk_all(|op| {
            if let Some(split) = op.downcast_ref::<Split>() {
                assert_eq!(
                    &*split.get_limb_ty(),
                    &Type::Felt,
                    "expected split to use `felt` limbs"
                );
                assert!(
                    split_hi.replace(split.limbs()[0].borrow().as_value_ref()).is_none(),
                    "expected a single arith.split"
                );
            }
            if let Some(join) = op.downcast_ref::<Join>() {
                assert_eq!(&*join.get_ty(), &Type::U64, "expected join to reconstruct u64");
                let [high, low] = join.limbs().as_slice() else {
                    panic!("expected arith.join to consume 2 limbs for u64");
                };
                let limbs = (high.borrow().as_value_ref(), low.borrow().as_value_ref());
                assert!(join_limbs.replace(limbs).is_none(), "expected a single arith.join");
            }
        });

        let split_hi = split_hi.expect("expected arith.split");
        let (high, low) = join_limbs.expect("expected arith.join");
        assert_eq!(low, split_hi, "expected join low limb to use split high limb");
        let high_op = high.borrow().get_defining_op().expect("expected a constant high limb");
        let high_op = high_op.borrow();
        let constant = high_op.downcast_ref::<Constant>().expect("expected arith.constant");
        assert_eq!(constant.value().as_felt(), Some(midenc_hir::Felt::ZERO));

        Ok(())
    }

    #[test]
    fn shr_i64_by_32_is_not_rewritten() -> Result<(), Report> {
        let mut test = Test::named("shr_i64_by_32_is_not_rewritten");
        build_shr(&mut test, Type::I64, 32)?;

        apply_shr_canonicalization(&test);
        assert!(contains_shr(&test), "arithmetic shift of i64 must be left alone");

        Ok(())
    }

    #[test]
    fn shr_u64_by_non_32_is_not_rewritten() -> Result<(), Report> {
        let mut test = Test::named("shr_u64_by_non_32_is_not_rewritten");
        build_shr(&mut test, Type::U64, 31)?;

        apply_shr_canonicalization(&test);
        assert!(contains_shr(&test), "shift by a count other than 32 must be left alone");

        Ok(())
    }

    #[test]
    fn inline_metadata_survives_shr_rewrite() -> Result<(), Report> {
        use midenc_hir::dialects::debuginfo::attributes::{
            INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr,
        };
        let mut test = Test::named("inline_metadata_survives_shr_rewrite");
        build_shr(&mut test, Type::U64, 32)?;
        let function = test.function().as_operation_ref();
        let mut shr = None;
        function.borrow().prewalk_all(|op| {
            if op.is::<Shr>() {
                shr = Some(op.as_operation_ref());
            }
        });
        let attr = test
            .context_rc()
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::default())
            .as_attribute_ref();
        shr.unwrap().borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, attr);

        assert!(apply_shr_canonicalization(&test));
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

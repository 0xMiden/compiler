use alloc::{boxed::Box, rc::Rc};

use super::{super::capture_invalidation::tests::render, *};
use crate::{
    FxHashSet, RegionKindInterface, SourceSpan, Type,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::{DebugKill, attributes::Variable},
        test::TestDialect,
    },
    patterns::{
        FrozenRewritePatternSet, Pattern, PatternBenefit, PatternInfo, PatternKind, RewritePattern,
        RewritePatternSet, RewriterImpl,
    },
    testing::Test,
};

#[test]
fn source_keys_use_semantic_frames_and_distinct_identity_relations() -> Result<(), Report> {
    use super::super::{
        attributes::{
            Expression, ExpressionOp, INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain,
            InlineCallChainAttr,
        },
        bindings::RecoveryIdentity,
    };
    let mut test = Test::new("identity", &[Type::U32], &[Type::U32]);
    let context = test.context_rc();
    let (mut first, mut second) = {
        let mut builder = test.function_builder();
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        let record = |builder: &mut crate::OpBuilder| {
            builder.debug_declare(
                Variable::new("x".into(), "test.rs".into(), 1, None),
                Expression {
                    operations: alloc::vec![ExpressionOp::LocalSlot(0)],
                },
                SourceSpan::UNKNOWN,
            )
        };
        let first = record(BuiltinOpBuilder::builder_mut(&mut builder))?.as_operation_ref();
        let second = record(BuiltinOpBuilder::builder_mut(&mut builder))?.as_operation_ref();
        builder.ret([value], SourceSpan::UNKNOWN)?;
        (first, second)
    };
    for record in [&mut first, &mut second] {
        let chain = context
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![]))
            .as_attribute_ref();
        record.borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
    }
    let a = SourceBinding::of(first).unwrap();
    let b = SourceBinding::of(second).unwrap();
    let mut seen = FxHashSet::default();
    seen.insert(a.clone());
    assert!(
        !seen.insert(b.clone()),
        "independently allocated equal frame attributes are one source identity"
    );
    second.borrow_mut().remove_attribute(INLINE_CALL_CHAIN_ATTR_NAME);
    assert_ne!(
        a,
        SourceBinding::of(second).unwrap(),
        "absent and explicit empty chains remain distinct"
    );
    let mut shadow = b.clone();
    shadow.identity.variable.line = 2;
    assert_ne!(a, shadow);
    assert_eq!(
        a.visibility(),
        shadow.visibility(),
        "different declarations may shadow one visible name"
    );
    let mut recovery = RecoveryIdentity::of(&first.borrow()).unwrap();
    let other = recovery.clone();
    recovery.expression = Some(Expression {
        operations: alloc::vec![ExpressionOp::LocalSlot(1)],
    });
    assert_ne!(recovery, other, "different locations cannot share recovery facts");
    assert_eq!(
        recovery.binding, other.binding,
        "changing recovery does not change source identity"
    );
    Ok(())
}

#[test]
fn greedy_capture_invalidation_output_is_bounded() -> Result<(), Report> {
    use crate::patterns::{GreedyRewriteConfig, apply_patterns_and_fold_greedily};
    struct EraseCapture(PatternInfo);
    impl Pattern for EraseCapture {
        fn info(&self) -> &PatternInfo {
            &self.0
        }
    }
    impl RewritePattern for EraseCapture {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            rewriter: &mut dyn Rewriter,
        ) -> Result<bool, Report> {
            if !op.name().is::<DebugValue>() {
                return Ok(false);
            }
            invalidate_debug_captures(op, rewriter)?;
            Ok(true)
        }
    }
    for audited in [false, true] {
        for top_down in [false, true] {
            for count in [16, 32] {
                for repeated in [false, true] {
                    let mut test = Test::new("bounded", &[Type::U32], &[Type::U32]);
                    {
                        let mut builder = test.function_builder();
                        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
                        for i in 0..count {
                            BuiltinOpBuilder::builder_mut(&mut builder).debug_value(
                                value,
                                Variable::new(
                                    alloc::format!("x{}", if repeated { i % 8 } else { i }).into(),
                                    "test.rs".into(),
                                    i + 1,
                                    None,
                                ),
                                SourceSpan::UNKNOWN,
                            )?;
                        }
                        builder.ret([value], SourceSpan::UNKNOWN)?;
                    }
                    let context = test.context_rc();
                    let root = test.function().as_operation_ref();
                    let audit =
                        audited.then(|| super::super::mutation::DebugInfoAudit::begin(root));
                    let pattern = EraseCapture(PatternInfo::new(
                        context.clone(),
                        "erase-capture",
                        PatternKind::Any,
                        PatternBenefit::MIN,
                    ));
                    let patterns =
                        Rc::new(FrozenRewritePatternSet::new(RewritePatternSet::from_iter(
                            context,
                            [Box::new(pattern) as Box<dyn RewritePattern>],
                        )));
                    let mut config = GreedyRewriteConfig::default();
                    config.with_top_down_traversal(top_down);
                    super::super::capture_invalidation::stats::COUNTS
                        .with(|counts| *counts.borrow_mut() = Default::default());
                    assert!(apply_patterns_and_fold_greedily(root, patterns, config).is_ok());
                    super::super::capture_invalidation::stats::COUNTS.with(|counts| {
                        let counts = counts.borrow();
                        assert_eq!(counts.requests, count as usize);
                        assert_eq!(counts.bindings, 0);
                        assert!(counts.nodes <= count as usize * 3 + 8, "{counts:?}");
                        assert!(counts.edges <= count as usize * 3 + 8, "{counts:?}");
                    });
                    if let Some(audit) = audit {
                        audit.finish()?;
                    }
                    let records = collect_debug_ops(&root);
                    assert!(records.iter().all(|op| op.name().is::<DebugKill>()));
                    assert_eq!(
                        records.len(),
                        count as usize,
                        "audited={audited}, top_down={top_down}, repeated={repeated}, \
                         count={count}"
                    );
                }
            }
        }
    }
    Ok(())
}

fn capture_fixture(lines: &[u32]) -> Result<(Test, Vec<OperationRef>, ValueRef), Report> {
    let mut test = Test::new("capture_phase", &[Type::U32], &[Type::U32]);
    let mut records = Vec::new();
    let value;
    {
        let mut builder = test.function_builder();
        value = builder.current_block().borrow().arguments()[0] as ValueRef;
        for &line in lines {
            records.push(
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_value(
                        value,
                        Variable::new("x".into(), "test.rs".into(), line, None),
                        SourceSpan::UNKNOWN,
                    )?
                    .as_operation_ref(),
            );
        }
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    Ok((test, records, value))
}
fn kill_lines(root: OperationRef) -> Vec<u32> {
    collect_debug_ops(&root)
        .into_iter()
        .filter(|op| op.name().is::<DebugKill>())
        .map(|op| debug_record_variable(&op.borrow()).unwrap().line)
        .collect()
}
fn phase(root: OperationRef) -> Result<CaptureInvalidationPhase, Report> {
    CaptureInvalidationPhase::begin(root, Some(Rc::new(crate::patterns::NoopRewriterListener)))
}

#[test]
fn surviving_bindings_stop_at_first_clear_and_restart_at_rebind() -> Result<(), Report> {
    for audited in [false, true] {
        for rebind in [false, true] {
            for reverse in [false, true] {
                let (test, records, _) =
                    capture_fixture(if rebind { &[1, 2, 1, 3] } else { &[1, 2, 3] })?;
                let root = test.function().as_operation_ref();
                let audit = audited.then(|| super::super::mutation::DebugInfoAudit::begin(root));
                let phase = phase(root)?;
                let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
                let mut removed = [records[1], *records.last().unwrap()];
                if reverse {
                    removed.reverse();
                }
                for record in removed {
                    invalidate_debug_captures(record, &mut rewriter)?;
                }
                assert!(phase.finish()?);
                assert_eq!(
                    kill_lines(root),
                    if rebind {
                        alloc::vec![1, 2, 1, 3]
                    } else {
                        alloc::vec![1, 2, 3]
                    }
                );
                if let Some(audit) = audit {
                    audit.finish()?;
                }
            }
        }
    }
    Ok(())
}

#[test]
fn current_flow_includes_new_bindings_and_respects_function_return() -> Result<(), Report> {
    for prior in [false, true] {
        let (test, records, value) = capture_fixture(&[2, 1])?;
        let root = test.function().as_operation_ref();
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(records[0], &mut rewriter)?;
        if prior {
            let anchor = collect_debug_ops(&root)[0];
            rewriter.set_insertion_point_before(anchor);
            rewriter.debug_value(
                value,
                Variable::new("x".into(), "test.rs".into(), 1, None),
                SourceSpan::UNKNOWN,
            )?;
            rewriter.debug_value(
                value,
                Variable::new("x".into(), "test.rs".into(), 3, None),
                SourceSpan::UNKNOWN,
            )?;
        }
        phase.finish()?;
        assert_eq!(
            kill_lines(root),
            if prior {
                alloc::vec![1, 3, 2]
            } else {
                alloc::vec![2]
            },
            "a later bind cannot reach backward through a function return"
        );
    }
    Ok(())
}

#[test]
fn pending_anchor_replacement_and_coalescence_are_checked_without_audit() -> Result<(), Report> {
    for audited in [false, true] {
        let (test, records, _) = capture_fixture(&[1, 2])?;
        let root = test.function().as_operation_ref();
        let audit = audited.then(|| super::super::mutation::DebugInfoAudit::begin(root));
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(records[1], &mut rewriter)?;
        let anchor = collect_debug_ops(&root)[1];
        assert!(is_pending_capture(anchor));
        {
            let mut update = DebugInfoMutation::for_operations(
                &[anchor, records[0]],
                DebugInfoUpdate::Equivalent,
            );
            assert!(update.coalesce_capture(anchor, records[0], &[]).is_err());
        }
        invalidate_debug_captures(anchor, &mut rewriter)?;
        assert!(!is_pending_capture(anchor));
        phase.finish()?;
        assert_eq!(kill_lines(root), [1, 2]);
        if let Some(audit) = audit {
            audit.finish()?;
        }
    }
    Ok(())
}

#[test]
fn orphaned_or_changed_pending_anchors_are_hard_failures() -> Result<(), Report> {
    for erase in [false, true] {
        let (test, records, _) = capture_fixture(&[1, 2])?;
        let root = test.function().as_operation_ref();
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(records[1], &mut rewriter)?;
        let mut anchor = collect_debug_ops(&root)[1];
        if erase {
            anchor.borrow_mut().erase();
        } else {
            let variable = test
                .context_rc()
                .create_attribute::<super::super::attributes::InlineCallChainAttr, _>(
                    super::super::attributes::InlineCallChain::new(alloc::vec![]),
                )
                .as_attribute_ref();
            anchor
                .borrow_mut()
                .set_attribute(super::super::attributes::INLINE_CALL_CHAIN_ATTR_NAME, variable);
        }
        assert!(phase.finish().is_err());
        assert!(!super::super::capture_invalidation::active(test.context()));
    }
    Ok(())
}

#[test]
fn nested_barrier_notifies_originating_listener_and_outer_phase_resumes() -> Result<(), Report> {
    use core::cell::Cell;

    use crate::{Listener, ProgramPoint};
    struct Insertions(Cell<usize>);
    impl Listener for Insertions {
        fn kind(&self) -> crate::ListenerType {
            crate::ListenerType::Rewriter
        }

        fn notify_operation_inserted(&self, _: OperationRef, _: ProgramPoint) {
            self.0.set(self.0.get() + 1);
        }
    }
    impl crate::patterns::RewriterListener for Insertions {}
    let (test, records, _) = capture_fixture(&[1, 2, 1, 3])?;
    let root = test.function().as_operation_ref();
    let outer_audit = super::super::mutation::DebugInfoAudit::begin(root);
    let listener = Rc::new(Insertions(Cell::new(0)));
    let outer = CaptureInvalidationPhase::begin(root, Some(listener.clone()))?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(records[1], &mut rewriter)?;
    let inner = phase(root)?;
    assert_eq!(listener.0.get(), 1);
    assert_eq!(kill_lines(root), [1, 2]);
    let inner_audit = super::super::mutation::DebugInfoAudit::begin(root);
    inner.finish()?;
    inner_audit.finish()?;
    invalidate_debug_captures(records[3], &mut rewriter)?;
    outer.finish()?;
    assert_eq!(listener.0.get(), 2);
    assert_eq!(kill_lines(root), [1, 2, 1, 3]);
    outer_audit.finish()?;
    Ok(())
}

#[test]
fn empty_phase_completion_does_not_scan_borrowed_ir() -> Result<(), Report> {
    let (test, ..) = capture_fixture(&[1])?;
    let root = test.function().as_operation_ref();
    let phase = phase(root)?;
    let mut root = root;
    let _borrow = root.borrow_mut();
    assert!(!phase.finish()?);
    Ok(())
}

#[test]
fn pending_discard_requires_its_container_to_disappear() -> Result<(), Report> {
    for discard_block in [false, true] {
        let (test, records, value) = capture_fixture(&[1])?;
        let root = test.function().as_operation_ref();
        let region = records[0].parent().unwrap().parent().unwrap();
        let mut builder = test.context_rc().builder();
        let mut block = builder.create_block(region, None, &[]);
        let record = builder
            .debug_value(
                value,
                Variable::new("x".into(), "test.rs".into(), 2, None),
                SourceSpan::UNKNOWN,
            )?
            .as_operation_ref();
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(record, &mut rewriter)?;
        {
            let _guard =
                DebugInfoMutation::for_blocks(&[block], DebugInfoUpdate::DiscardUnreachable);
            if discard_block {
                block.borrow_mut().erase();
            } else {
                let mut anchor = block.borrow().body().front().as_pointer().unwrap();
                anchor.borrow_mut().erase();
            }
        }
        assert_eq!(phase.finish().is_ok(), discard_block);
        assert!(!super::super::capture_invalidation::active(test.context()));
    }
    Ok(())
}

#[test]
fn greedy_budget_and_materialization_failures_have_distinct_results() -> Result<(), Report> {
    use crate::patterns::{
        ApplyPatternsAndFoldEffect, GreedyRewriteConfig, GreedyRewriteError,
        apply_patterns_and_fold,
    };
    struct Rewrite {
        info: PatternInfo,
        break_anchor: bool,
    }
    impl Pattern for Rewrite {
        fn info(&self) -> &PatternInfo {
            &self.info
        }
    }
    impl RewritePattern for Rewrite {
        fn match_and_rewrite(
            &self,
            op: OperationRef,
            rewriter: &mut dyn Rewriter,
        ) -> Result<bool, Report> {
            if !op.name().is::<DebugValue>() {
                return Ok(false);
            }
            let next = op.next().unwrap();
            invalidate_debug_captures(op, rewriter)?;
            if self.break_anchor {
                next.prev().unwrap().borrow_mut().erase();
            }
            Ok(true)
        }
    }
    for break_anchor in [false, true] {
        let (test, records, _) = capture_fixture(&[1, 2, 3])?;
        let context = test.context_rc();
        let root = test.function().as_operation_ref();
        let pattern = Rewrite {
            info: PatternInfo::new(
                context.clone(),
                "budget-capture",
                PatternKind::Any,
                PatternBenefit::MIN,
            ),
            break_anchor,
        };
        let patterns = Rc::new(FrozenRewritePatternSet::new(RewritePatternSet::from_iter(
            context,
            [Box::new(pattern) as Box<dyn RewritePattern>],
        )));
        let mut config = GreedyRewriteConfig::default();
        config.with_max_rewrites(1);
        let result = apply_patterns_and_fold(&records[1..], patterns, config);
        if break_anchor {
            assert!(matches!(result, Err(GreedyRewriteError::Failed(_))));
        } else {
            assert!(matches!(
                result,
                Err(GreedyRewriteError::NonConverged(ApplyPatternsAndFoldEffect::Changed))
            ));
            assert!(!kill_lines(root).is_empty());
        }
        assert!(!super::super::capture_invalidation::active(test.context()));
    }
    Ok(())
}

#[test]
fn nested_pass_direct_helper_finishes_before_return_on_success_and_error() -> Result<(), Report> {
    use crate::{
        EntityMut, OperationName,
        dialects::builtin::Function,
        pass::{Pass, PassExecutionState},
    };
    struct Nested(bool);
    impl Pass for Nested {
        type Target = Function;

        fn name(&self) -> &'static str {
            "nested-capture"
        }

        fn can_schedule_on(&self, _: &OperationName) -> bool {
            true
        }

        fn run_on_operation(
            &mut self,
            op: EntityMut<'_, Function>,
            _: &mut PassExecutionState,
        ) -> Result<(), Report> {
            let root = op.as_operation_ref();
            let context = op.as_operation().context_rc();
            drop(op);
            let records = collect_debug_ops(&root);
            let mut rewriter: RewriterImpl = RewriterImpl::new(context);
            invalidate_debug_captures(records[1], &mut rewriter)?;
            if self.0 {
                Err(Report::msg("deliberate nested pass failure"))
            } else {
                Ok(())
            }
        }
    }
    for error in [false, true] {
        let (test, ..) = capture_fixture(&[1, 2])?;
        let root = test.function().as_operation_ref();
        let outer = phase(root)?;
        assert_eq!(test.apply_boxed_pass(Box::new(Nested(error)), false).is_err(), error);
        assert_eq!(kill_lines(root), [1, 2], "nested pass returned with unresolved outer requests");
        outer.finish()?;
    }
    Ok(())
}

#[crate::derive::operation(dialect = TestDialect)]
pub struct OpaqueCaptureOwner {
    #[region]
    body: crate::Region,
}
#[crate::derive::operation(dialect = TestDialect, implements(RegionKindInterface))]
pub struct GraphCaptureOwner {
    #[region]
    body: crate::Region,
}
impl crate::RegionKindInterface for GraphCaptureOwner {
    fn kind(&self) -> crate::RegionKind {
        crate::RegionKind::Graph
    }
}

#[test]
fn opaque_and_graph_regions_conservatively_repeat_in_linear_space() -> Result<(), Report> {
    use crate::BuilderExt;
    for graph in [false, true] {
        for count in [8, 16] {
            let (test, records, value) = capture_fixture(&[1])?;
            let root = test.function().as_operation_ref();
            let mut builder = test.context_rc().builder();
            builder.set_insertion_point_before(records[0]);
            let mut owner = if graph {
                builder.create::<GraphCaptureOwner, ()>(SourceSpan::UNKNOWN)()?.as_operation_ref()
            } else {
                builder.create::<OpaqueCaptureOwner, ()>(SourceSpan::UNKNOWN)()?.as_operation_ref()
            };
            for _ in 1..count {
                owner.borrow_mut().regions_mut().push_back(test.context_rc().create_region());
            }
            let regions =
                owner.borrow().regions().iter().map(|r| r.as_region_ref()).collect::<Vec<_>>();
            let mut removed = None;
            for (i, region) in regions.into_iter().enumerate() {
                builder.create_block(region, None, &[]);
                if i == 0 {
                    removed = Some(
                        builder
                            .debug_value(
                                value,
                                Variable::new("x".into(), "test.rs".into(), 2, None),
                                SourceSpan::UNKNOWN,
                            )?
                            .as_operation_ref(),
                    );
                }
                builder.debug_value(
                    value,
                    Variable::new("x".into(), "test.rs".into(), 3, None),
                    SourceSpan::UNKNOWN,
                )?;
            }
            let phase = phase(root)?;
            let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
            invalidate_debug_captures(removed.unwrap(), &mut rewriter)?;
            super::super::capture_invalidation::stats::COUNTS
                .with(|counts| *counts.borrow_mut() = Default::default());
            phase.finish()?;
            assert_eq!(kill_lines(root), [3, 2]);
            super::super::capture_invalidation::stats::COUNTS.with(|counts| {
                assert!(
                    counts.borrow().edges < count * 12 + 20,
                    "unknown region flow must use a linear hub: {:?}",
                    counts.borrow()
                );
            });
        }
    }
    Ok(())
}

#[test]
fn pending_relocation_uses_final_capture_position() -> Result<(), Report> {
    for audited in [false, true] {
        let (test, records, _) = capture_fixture(&[2, 1])?;
        let root = test.function().as_operation_ref();
        let audit = audited.then(|| super::super::mutation::DebugInfoAudit::begin(root));
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(records[0], &mut rewriter)?;
        let anchor = collect_debug_ops(&root)[0];
        {
            let _guard = DebugInfoMutation::new(anchor, DebugInfoUpdate::Relocate);
            rewriter.move_op_after(anchor, records[1]);
        }
        phase.finish()?;
        assert_eq!(kill_lines(root), [1, 2]);
        if let Some(audit) = audit {
            audit.finish()?;
        }
    }
    Ok(())
}

#[test]
fn inline_frames_are_isolated_and_equal_chains_share_flow() -> Result<(), Report> {
    use super::super::attributes::{
        INLINE_CALL_CHAIN_ATTR_NAME, InlineCallChain, InlineCallChainAttr,
    };
    for equal in [false, true] {
        let (test, mut records, _) = capture_fixture(&[1, 2])?;
        let root = test.function().as_operation_ref();
        let context = test.context_rc();
        let chain = context
            .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![]))
            .as_attribute_ref();
        records[0].borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
        if equal {
            let chain = context
                .create_attribute::<InlineCallChainAttr, _>(InlineCallChain::new(alloc::vec![]))
                .as_attribute_ref();
            records[1].borrow_mut().set_attribute(INLINE_CALL_CHAIN_ATTR_NAME, chain);
        }
        let audit = super::super::mutation::DebugInfoAudit::begin(root);
        let phase = phase(root)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(context);
        invalidate_debug_captures(records[1], &mut rewriter)?;
        phase.finish()?;
        assert_eq!(
            kill_lines(root),
            if equal {
                alloc::vec![1, 2]
            } else {
                alloc::vec![2]
            }
        );
        audit.finish()?;
    }
    Ok(())
}

#[test]
fn shared_phase_keeps_functions_separate_and_discards_whole_function() -> Result<(), Report> {
    use crate::dialects::builtin::FunctionBuilder;
    for discard in [false, true] {
        let mut test = Test::named("functions").in_module("test");
        test.with_function("first", &[Type::U32], &[Type::U32]);
        let first = test.function();
        let second = test.define_function("second", &[Type::U32], &[Type::U32]);
        let module = first.as_operation_ref().parent_op().unwrap();
        let context = test.context_rc();
        let mut records = Vec::new();
        for (function, line) in [(first, 1), (second, 2)] {
            let mut builder = context.clone().builder();
            let mut builder = FunctionBuilder::new(function, &mut builder);
            let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
            records.push(
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_value(
                        value,
                        Variable::new("x".into(), "test.rs".into(), line, None),
                        SourceSpan::UNKNOWN,
                    )?
                    .as_operation_ref(),
            );
            builder.ret([value], SourceSpan::UNKNOWN)?;
        }
        let audit = super::super::mutation::DebugInfoAudit::begin(module);
        let phase = phase(module)?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(context);
        invalidate_debug_captures(records[1], &mut rewriter)?;
        if discard {
            rewriter.erase_op_with_debug(
                second.as_operation_ref(),
                DebugInfoUpdate::DiscardUnreachable,
            );
        }
        phase.finish()?;
        assert_eq!(
            kill_lines(module),
            if discard {
                alloc::vec![]
            } else {
                alloc::vec![2]
            }
        );
        audit.finish()?;
    }
    Ok(())
}

#[test]
fn discard_cannot_cancel_a_pending_anchor_moved_into_another_live_function() -> Result<(), Report> {
    use crate::dialects::builtin::FunctionBuilder;
    let mut test = Test::named("functions").in_module("test");
    test.with_function("first", &[Type::U32], &[Type::U32]);
    let first = test.function();
    let second = test.define_function("second", &[Type::U32], &[Type::U32]);
    let context = test.context_rc();
    let record;
    {
        let mut builder = context.clone().builder();
        let mut builder = FunctionBuilder::new(second, &mut builder);
        let value = builder.entry_block().borrow().arguments()[0] as ValueRef;
        record = BuiltinOpBuilder::builder_mut(&mut builder)
            .debug_value(
                value,
                Variable::new("x".into(), "test.rs".into(), 1, None),
                SourceSpan::UNKNOWN,
            )?
            .as_operation_ref();
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let root = first.as_operation_ref().parent_op().unwrap();
    let phase = phase(root)?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(context);
    invalidate_debug_captures(record, &mut rewriter)?;
    let source = second.borrow().entry_block();
    let destination = first.borrow().entry_block();
    {
        let _guard = DebugInfoMutation::for_blocks(&[source], DebugInfoUpdate::DiscardUnreachable);
        rewriter.move_block_before(source, destination);
    }
    assert!(phase.finish().is_err());
    Ok(())
}

#[test]
fn raw_function_detach_does_not_discard_pending_obligations() -> Result<(), Report> {
    let (test, records, _) = capture_fixture(&[1, 2])?;
    let context = test.context_rc();
    let mut root = test.function().as_operation_ref();
    let mut builder = context.clone().builder();
    let module = builder.create_module(crate::Ident::with_empty_span("module".into()))?;
    let region = module.borrow().body().as_region_ref();
    let mut block = builder.create_block(region, None, &[]);
    block.borrow_mut().body_mut().push_back(root);
    let mut rewriter: RewriterImpl = RewriterImpl::new(context);
    let phase = phase(root)?;
    invalidate_debug_captures(records[1], &mut rewriter)?;
    root.borrow_mut().erase();
    assert!(phase.finish().is_err());
    Ok(())
}

#[test]
fn removing_old_owner_does_not_discard_a_pending_anchor_that_escaped() -> Result<(), Report> {
    use crate::{BuilderExt, dialects::builtin::FunctionBuilder};
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Owner {
        Function,
        NestedOperation,
        Block,
    }
    for audited in [false, true] {
        for owner_kind in [Owner::Function, Owner::NestedOperation, Owner::Block] {
            let mut test = Test::named("escaped_capture").in_module("test");
            test.with_function("destination", &[Type::U32], &[Type::U32]);
            let destination = test.function();
            let source = test.define_function("source", &[Type::U32], &[Type::U32]);
            let context = test.context_rc();
            let mut records = Vec::new();
            let mut nested_owner = None;
            for (function, line) in [(destination, 1), (source, 2)] {
                let mut builder = context.clone().builder();
                let mut builder = FunctionBuilder::new(function, &mut builder);
                let entry = builder.entry_block();
                let value = entry.borrow().arguments()[0] as ValueRef;
                if function == source && owner_kind == Owner::NestedOperation {
                    let owner = BuiltinOpBuilder::builder_mut(&mut builder)
                        .create::<OpaqueCaptureOwner, ()>(SourceSpan::UNKNOWN)(
                    )?
                    .as_operation_ref();
                    let region = owner.borrow().regions().front().as_pointer().unwrap();
                    let block = builder.create_block_in_region(region);
                    builder.switch_to_block(block);
                    nested_owner = Some(owner);
                }
                records.push(
                    BuiltinOpBuilder::builder_mut(&mut builder)
                        .debug_value(
                            value,
                            Variable::new("x".into(), "test.rs".into(), line, None),
                            SourceSpan::UNKNOWN,
                        )?
                        .as_operation_ref(),
                );
                builder.switch_to_block(entry);
                builder.ret([value], SourceSpan::UNKNOWN)?;
            }
            let root = destination.as_operation_ref().parent_op().unwrap();
            let audit = audited.then(|| super::super::mutation::DebugInfoAudit::begin(root));
            let phase = phase(root)?;
            let mut rewriter: RewriterImpl = RewriterImpl::new(context);
            invalidate_debug_captures(records[1], &mut rewriter)?;
            let anchor = collect_debug_ops(&source.as_operation_ref())[0];
            let mut source_block = source.borrow().entry_block();
            let owner = nested_owner.unwrap_or(source.as_operation_ref());
            {
                let _guard = if owner_kind == Owner::Block {
                    DebugInfoMutation::for_blocks(
                        &[source_block],
                        DebugInfoUpdate::DiscardUnreachable,
                    )
                    .unwrap()
                } else {
                    DebugInfoMutation::new(owner, DebugInfoUpdate::DiscardUnreachable)
                };
                rewriter.move_op_after(anchor, records[0]);
                if owner_kind == Owner::Block {
                    source_block.borrow_mut().erase();
                } else {
                    rewriter.erase_op(owner);
                }
            }
            assert_eq!(
                SourceBinding::of(anchor).unwrap().function,
                Some(destination.as_operation_ref())
            );
            assert!(
                phase.finish().is_err(),
                "escaped anchor was canceled: audited={audited}, owner_kind={owner_kind:?}"
            );
            if let Some(audit) = audit {
                assert!(audit.finish().is_err());
            }
        }
    }
    Ok(())
}

fn check_retained_phase_after_root_erasure(
    body_error: bool,
    outer_phase: bool,
) -> Result<(), Report> {
    use core::cell::RefCell;

    use crate::{
        BuilderExt, EntityMut, OperationName,
        dialects::builtin::Function,
        pass::{Pass, PassExecutionState},
    };

    struct RetainPhase {
        owner: OperationRef,
        retained: Rc<RefCell<Option<CaptureInvalidationPhase>>>,
        body_error: bool,
    }
    impl Pass for RetainPhase {
        type Target = Function;

        fn name(&self) -> &'static str {
            "retained-capture-phase"
        }

        fn can_schedule_on(&self, _: &OperationName) -> bool {
            true
        }

        fn run_on_operation(
            &mut self,
            op: EntityMut<'_, Function>,
            _: &mut PassExecutionState,
        ) -> Result<(), Report> {
            let context = op.as_operation().context_rc();
            drop(op);
            *self.retained.borrow_mut() = Some(phase(self.owner)?);
            let mut rewriter: RewriterImpl = RewriterImpl::new(context);
            invalidate_debug_captures(self.owner, &mut rewriter)?;
            rewriter.erase_op_with_debug(self.owner, DebugInfoUpdate::Invalidate);
            if self.body_error {
                Err(Report::msg("deliberate retained-phase pass failure"))
            } else {
                Ok(())
            }
        }
    }

    let (test, records, value) = capture_fixture(&[1, 3])?;
    let root = test.function().as_operation_ref();
    let mut builder = test.context_rc().builder();
    builder.set_insertion_point_before(records[1]);
    let owner = builder.create::<OpaqueCaptureOwner, ()>(SourceSpan::UNKNOWN)()?.as_operation_ref();
    let region = owner.borrow().regions().front().as_pointer().unwrap();
    builder.create_block(region, None, &[]);
    builder.debug_value(
        value,
        Variable::new("x".into(), "test.rs".into(), 2, None),
        SourceSpan::UNKNOWN,
    )?;
    let outer = outer_phase.then(|| phase(root)).transpose()?;
    let retained = Rc::new(RefCell::new(None));
    let result = test.apply_boxed_pass(
        Box::new(RetainPhase {
            owner,
            retained: retained.clone(),
            body_error,
        }),
        false,
    );

    // The post-body barrier successfully materialized the valid exact replacement, while the
    // phase handle is still alive and its original root is detached.
    assert!(retained.borrow().is_some());
    assert!(owner.parent_op().is_none());
    assert_eq!(kill_lines(root), [1, 2]);
    let error = result.expect_err("audit accepted an unfinished phase after its root was erased");
    let message = render(&error);
    assert!(message.contains("capture invalidation phase outlived its audit"), "{message}");
    if body_error {
        assert!(message.contains("deliberate retained-phase pass failure"), "{message}");
    }
    assert_eq!(super::super::capture_invalidation::active(test.context()), outer_phase);
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    assert!(invalidate_debug_captures(records[1], &mut rewriter).is_err());
    assert_eq!(kill_lines(root), [1, 2], "expired phase accepted a later helper request");
    assert!(records[1].parent_op().is_some());
    assert!(retained.borrow_mut().take().unwrap().finish().is_err());
    if let Some(outer) = outer {
        assert!(outer.finish().is_err());
    }
    Ok(())
}

#[test]
fn audit_rejects_retained_phase_after_root_erasure() -> Result<(), Report> {
    for outer_phase in [false, true] {
        check_retained_phase_after_root_erasure(false, outer_phase)?;
    }
    Ok(())
}

#[test]
fn failed_pass_retires_retained_phase_after_root_erasure() -> Result<(), Report> {
    for outer_phase in [false, true] {
        check_retained_phase_after_root_erasure(true, outer_phase)?;
    }
    Ok(())
}

#[test]
fn capture_failure_is_reported_once_through_nested_pass_cleanup() -> Result<(), Report> {
    use core::cell::RefCell;

    use crate::{
        EntityMut, OperationName,
        dialects::builtin::Function,
        pass::{Nesting, OpPassManager, Pass, PassExecutionState},
    };

    const ROOT_FAILURE: &str =
        "pending capture anchor erased, detached, or changed without a mapping";
    struct FailCapture {
        depth: usize,
        corrupt: bool,
        body: Option<&'static str>,
        retained: Rc<RefCell<Option<CaptureInvalidationPhase>>>,
    }
    impl Pass for FailCapture {
        type Target = Function;

        fn name(&self) -> &'static str {
            if self.depth == 0 {
                "capture-diagnostic-fixture"
            } else {
                "outer-capture-pipeline"
            }
        }

        fn can_schedule_on(&self, _: &OperationName) -> bool {
            true
        }

        fn run_on_operation(
            &mut self,
            op: EntityMut<'_, Function>,
            state: &mut PassExecutionState,
        ) -> Result<(), Report> {
            let root = op.as_operation_ref();
            let context = op.as_operation().context_rc();
            drop(op);
            if self.depth > 0 {
                let mut pipeline =
                    OpPassManager::new(OpPassManager::ANY, Nesting::Explicit, context);
                pipeline.add_pass(Box::new(Self {
                    depth: self.depth - 1,
                    corrupt: self.corrupt,
                    body: self.body,
                    retained: self.retained.clone(),
                }));
                return state.run_pipeline(&mut pipeline, root);
            }
            if self.corrupt {
                *self.retained.borrow_mut() = Some(phase(root)?);
                let record = collect_debug_ops(&root)[1];
                let next = record.next().unwrap();
                let mut rewriter: RewriterImpl = RewriterImpl::new(context);
                invalidate_debug_captures(record, &mut rewriter)?;
                next.prev().unwrap().borrow_mut().erase();
            }
            if let Some(body) = self.body {
                return Err(Report::msg(body));
            }
            self.retained.borrow_mut().take().unwrap().finish()?;
            Ok(())
        }
    }
    // Dropping independent errors by their text would lose the last case's body failure.
    for depth in [0, 2] {
        for (corrupt, body, expected_roots) in [
            (false, Some("independent pass body failure"), 0),
            (true, None, 1),
            (true, Some("independent pass body failure"), 1),
            (true, Some(ROOT_FAILURE), 2),
        ] {
            let (test, ..) = capture_fixture(&[1, 2, 3])?;
            let retained = Rc::new(RefCell::new(None));
            let error = test
                .apply_boxed_pass(
                    Box::new(FailCapture {
                        depth,
                        corrupt,
                        body,
                        retained: retained.clone(),
                    }),
                    false,
                )
                .expect_err("failing pass must remain fatal without verification");
            let message = render(&error);
            assert_eq!(message.matches(ROOT_FAILURE).count(), expected_roots, "{message}");
            if let Some(body) = body {
                assert!(message.contains(body), "{message}");
            }
            if corrupt {
                assert!(message.contains("capture-diagnostic-fixture"), "{message}");
                if depth > 0 {
                    assert!(message.contains("outer-capture-pipeline"), "{message}");
                }
                if body.is_some() {
                    assert_eq!(
                        message.matches("capture invalidation phase outlived its audit").count(),
                        1,
                        "{message}"
                    );
                }
                assert!(capture_invalidation_barrier(&test.context_rc()).is_err());
                assert!(!super::super::capture_invalidation::active(test.context()));
                if let Some(phase) = retained.borrow_mut().take() {
                    assert!(phase.finish().is_err());
                }
            }
        }
    }
    Ok(())
}

#[test]
fn capture_failure_preserves_independent_out_of_order_cleanup() -> Result<(), Report> {
    let (test, records, _) = capture_fixture(&[1, 2])?;
    let root = test.function().as_operation_ref();
    let outer = phase(root)?;
    let inner = phase(root)?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(records[1], &mut rewriter)?;
    collect_debug_ops(&root)[1].borrow_mut().erase();
    let _ = capture_invalidation_barrier(&test.context_rc()).unwrap_err();
    let error = outer.finish().unwrap_err();
    let message = render(&error);
    assert_eq!(message.matches("pending capture anchor").count(), 1, "{message}");
    assert_eq!(
        message.matches("capture invalidation phases completed out of order").count(),
        1,
        "{message}"
    );
    assert!(inner.finish().is_err());
    assert!(!super::super::capture_invalidation::active(test.context()));
    Ok(())
}

#[test]
fn structured_pass_body_survives_capture_cleanup_with_provenance() -> Result<(), Report> {
    use core::cell::RefCell;

    use super::super::capture_invalidation::tests::{assert_structured, render, structured_report};
    use crate::{
        EntityMut, OperationName,
        dialects::builtin::Function,
        pass::{Pass, PassExecutionState},
    };
    struct FailStructured(Rc<RefCell<Option<CaptureInvalidationPhase>>>);
    impl Pass for FailStructured {
        type Target = Function;

        fn name(&self) -> &'static str {
            "structured-capture-pass"
        }

        fn can_schedule_on(&self, _: &OperationName) -> bool {
            true
        }

        fn run_on_operation(
            &mut self,
            op: EntityMut<'_, Function>,
            _: &mut PassExecutionState,
        ) -> Result<(), Report> {
            let root = op.as_operation_ref();
            let context = op.as_operation().context_rc();
            drop(op);
            *self.0.borrow_mut() = Some(phase(root)?);
            let record = collect_debug_ops(&root)[1];
            let next = record.next().unwrap();
            let mut rewriter: RewriterImpl = RewriterImpl::new(context);
            invalidate_debug_captures(record, &mut rewriter)?;
            next.prev().unwrap().borrow_mut().erase();
            Err(structured_report())
        }
    }
    let (test, ..) = capture_fixture(&[1, 2, 3])?;
    let retained = Rc::new(RefCell::new(None));
    let error = test
        .apply_boxed_pass(Box::new(FailStructured(retained.clone())), false)
        .unwrap_err();
    assert_structured(&error);
    let body = error
        .related()
        .unwrap()
        .find(|diagnostic| {
            diagnostic
                .code()
                .is_some_and(|code| alloc::format!("{code}") == "capture::structured")
        })
        .expect("structured body must remain exposed");
    let body_message = alloc::format!("{body}");
    assert_eq!(
        body_message.matches("pass 'structured-capture-pass'").count(),
        1,
        "{body_message}"
    );
    let rendered = render(&error);
    assert!(rendered.contains("structured-capture-pass"), "{rendered}");
    assert_eq!(rendered.matches("pending capture anchor erased").count(), 1, "{rendered}");
    assert_eq!(
        rendered.matches("capture invalidation phase outlived its audit").count(),
        1,
        "{rendered}"
    );
    assert!(retained.borrow_mut().take().unwrap().finish().is_err());
    Ok(())
}

#[test]
fn replacement_failure_retains_prior_guard_poison_and_its_own_cause() -> Result<(), Report> {
    let (test, records, _) = capture_fixture(&[1, 2])?;
    let root = test.function().as_operation_ref();
    let phase = phase(root)?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(records[1], &mut rewriter)?;
    let mut anchor = collect_debug_ops(&root)[1];
    let chain = test
        .context_rc()
        .create_attribute::<super::super::attributes::InlineCallChainAttr, _>(
            super::super::attributes::InlineCallChain::new(alloc::vec![]),
        )
        .as_attribute_ref();
    anchor
        .borrow_mut()
        .set_attribute(super::super::attributes::INLINE_CALL_CHAIN_ATTR_NAME, chain);
    let error = invalidate_debug_captures(anchor, &mut rewriter).unwrap_err();
    let error = combine_capture_errors(error, phase.finish().unwrap_err());
    let rendered = render(&error);
    assert_eq!(
        rendered.matches("pending capture changed before its mutation guard").count(),
        1,
        "{rendered}"
    );
    assert_eq!(rendered.matches("invalid pending capture replacement").count(), 1, "{rendered}");
    assert!(!super::super::capture_invalidation::active(test.context()));
    Ok(())
}

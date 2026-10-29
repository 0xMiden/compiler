use alloc::{boxed::Box, rc::Rc};
use core::cell::Cell;

use super::*;
use crate::{
    Context, SourceSpan, Type,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::{DebugKill, attributes::Variable},
    },
    patterns::{
        FrozenRewritePatternSet, Pattern, PatternApplicator, PatternBenefit, PatternInfo,
        PatternKind, RewritePattern, RewritePatternSet, RewriterImpl,
    },
    testing::Test,
};

std::thread_local! { pub(super) static BUILDS: Cell<usize> = const { Cell::new(0) }; }

#[derive(Clone, Copy)]
enum Action {
    Invalidate,
    Introduce,
    NoMatch,
    Error,
}
struct CapturePattern {
    info: PatternInfo,
    action: Action,
}
impl Pattern for CapturePattern {
    fn info(&self) -> &PatternInfo {
        &self.info
    }

    fn preserves_debug_bindings(&self) -> bool {
        matches!(self.action, Action::Invalidate)
    }
}
impl RewritePattern for CapturePattern {
    fn match_and_rewrite(
        &self,
        op: OperationRef,
        rewriter: &mut dyn Rewriter,
    ) -> Result<bool, Report> {
        match self.action {
            Action::Invalidate => {
                invalidate_debug_captures(op, rewriter)?;
                Ok(true)
            }
            Action::Introduce => {
                let value =
                    op.borrow().downcast_ref::<DebugValue>().unwrap().value().as_value_ref();
                rewriter.set_insertion_point_before(op);
                rewriter.debug_value(
                    value,
                    Variable::new("x".into(), "test.rs".into(), 2, None),
                    SourceSpan::UNKNOWN,
                )?;
                Ok(true)
            }
            Action::NoMatch => Ok(false),
            Action::Error => Err(Report::msg("deliberate rewrite error")),
        }
    }
}

fn apply(context: Rc<Context>, rewriter: &mut RewriterImpl, op: OperationRef, action: Action) {
    let pattern = CapturePattern {
        info: PatternInfo::new(
            context.clone(),
            "capture-test",
            PatternKind::Any,
            PatternBenefit::MIN,
        ),
        action,
    };
    let patterns =
        RewritePatternSet::from_iter(context, [Box::new(pattern) as Box<dyn RewritePattern>]);
    let mut applicator = PatternApplicator::new(Rc::new(FrozenRewritePatternSet::new(patterns)));
    applicator.apply_default_cost_model();
    let result = applicator.match_and_rewrite(op, rewriter, |_| true, |_| {}, |_| Ok(()));
    assert_eq!(result.is_ok(), !matches!(action, Action::NoMatch | Action::Error));
}

#[test]
fn inventory_reuse_tracks_successful_source_identity_changes() -> Result<(), Report> {
    let mut test = Test::new("inventory", &[Type::U32], &[Type::U32]);
    let mut records = Vec::new();
    {
        let mut builder = test.function_builder();
        let value = builder.current_block().borrow().arguments()[0] as ValueRef;
        for _ in 0..4 {
            records.push(
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_value(
                        value,
                        Variable::new("x".into(), "test.rs".into(), 1, None),
                        SourceSpan::UNKNOWN,
                    )?
                    .as_operation_ref(),
            );
        }
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let context = test.context_rc();
    let mut rewriter = RewriterImpl::new(context.clone());
    rewriter.debug_binding_inventory_cache().unwrap().enable();
    BUILDS.set(0);
    apply(context.clone(), &mut rewriter, records[0], Action::Invalidate);
    apply(context.clone(), &mut rewriter, records[1], Action::NoMatch);
    apply(context.clone(), &mut rewriter, records[1], Action::Invalidate);
    assert_eq!(BUILDS.get(), 1, "nonmatches must not defeat inventory reuse");
    apply(context.clone(), &mut rewriter, records[2], Action::Introduce);
    apply(context.clone(), &mut rewriter, records[2], Action::Invalidate);
    assert_eq!(BUILDS.get(), 2, "new source identities require a fresh inventory");
    let kills = collect_debug_ops(&test.function().as_operation_ref())
        .into_iter()
        .filter(|op| op.borrow().is::<DebugKill>())
        .map(|op| debug_record_variable(&op.borrow()).unwrap().line)
        .collect::<Vec<_>>();
    assert_eq!(kills, [1, 1, 1, 2], "the introduced shadow must also be invalidated");
    apply(context.clone(), &mut rewriter, records[3], Action::Error);
    apply(context, &mut rewriter, records[3], Action::Invalidate);
    assert_eq!(BUILDS.get(), 3, "error paths must discard the reusable inventory");
    Ok(())
}

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
    assert_eq!(DebugBindingInventory::new(test.function().as_operation_ref()).bindings.len(), 1);
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

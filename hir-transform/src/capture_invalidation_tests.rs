use alloc::{rc::Rc, vec::Vec};

use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    OperationRef, Report, SourceSpan, Type, ValueRef,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::{
            DIBuilder, DebugInfoUpdate, DebugKill,
            attributes::Variable,
            debug_record_variable,
            transform::{CaptureInvalidationPhase, collect_debug_ops, invalidate_debug_captures},
        },
    },
    patterns::{NoopRewriterListener, Rewriter, RewriterImpl},
    testing::Test,
};

fn record(
    builder: &mut midenc_hir::OpBuilder,
    value: ValueRef,
    line: u32,
) -> Result<OperationRef, Report> {
    Ok(builder
        .debug_value(
            value,
            Variable::new("x".into(), "test.rs".into(), line, None),
            SourceSpan::UNKNOWN,
        )?
        .as_operation_ref())
}
fn lines(root: OperationRef) -> Vec<u32> {
    collect_debug_ops(&root)
        .into_iter()
        .filter(|op| op.name().is::<DebugKill>())
        .map(|op| debug_record_variable(&op.borrow()).unwrap().line)
        .collect()
}

#[test]
fn structured_backedge_carries_later_binding_to_earlier_clear() -> Result<(), Report> {
    let mut test = Test::new("capture_loop", &[Type::I1, Type::U32], &[Type::U32]);
    let removed;
    {
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let condition = entry.borrow().arguments()[0] as ValueRef;
        let value = entry.borrow().arguments()[1] as ValueRef;
        let owner = builder.r#while([], &[], SourceSpan::UNKNOWN)?;
        let before = owner.borrow().before().entry().as_block_ref();
        let after = owner.borrow().after().entry().as_block_ref();
        builder.switch_to_block(before);
        removed = record(BuiltinOpBuilder::builder_mut(&mut builder), value, 2)?;
        builder.condition(condition, [], SourceSpan::UNKNOWN)?;
        builder.switch_to_block(after);
        record(BuiltinOpBuilder::builder_mut(&mut builder), value, 1)?;
        builder.r#yield([], SourceSpan::UNKNOWN)?;
        builder.switch_to_block(entry);
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let root = test.function().as_operation_ref();
    let phase = CaptureInvalidationPhase::begin(root, Some(Rc::new(NoopRewriterListener)))?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(removed, &mut rewriter)?;
    phase.finish()?;
    assert_eq!(lines(root), [1, 2]);
    Ok(())
}

#[test]
fn nested_owner_transfers_pending_anchor_and_merges_duplicate_originals() -> Result<(), Report> {
    let mut test = Test::new("nested_capture", &[Type::I1, Type::U32], &[Type::U32]);
    let owner;
    let mut captures = Vec::new();
    {
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let condition = entry.borrow().arguments()[0] as ValueRef;
        let value = entry.borrow().arguments()[1] as ValueRef;
        record(BuiltinOpBuilder::builder_mut(&mut builder), value, 1)?;
        owner = builder.r#if(condition, &[], SourceSpan::UNKNOWN)?.as_operation_ref();
        let regions =
            owner.borrow().regions().iter().map(|r| r.as_region_ref()).collect::<Vec<_>>();
        for region in regions {
            let block = builder.create_block_in_region(region);
            builder.switch_to_block(block);
            captures.push(record(BuiltinOpBuilder::builder_mut(&mut builder), value, 2)?);
            builder.r#yield([], SourceSpan::UNKNOWN)?;
        }
        builder.switch_to_block(entry);
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let root = test.function().as_operation_ref();
    let phase = CaptureInvalidationPhase::begin(root, Some(Rc::new(NoopRewriterListener)))?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(captures[0], &mut rewriter)?;
    invalidate_debug_captures(owner, &mut rewriter)?;
    rewriter.erase_op_with_debug(owner, DebugInfoUpdate::Invalidate);
    phase.finish()?;
    assert_eq!(lines(root), [1, 2]);
    Ok(())
}

#[test]
fn structured_join_unions_captures_from_both_predecessors() -> Result<(), Report> {
    let mut test = Test::new("capture_join", &[Type::I1, Type::U32], &[Type::U32]);
    let removed;
    {
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let condition = entry.borrow().arguments()[0] as ValueRef;
        let value = entry.borrow().arguments()[1] as ValueRef;
        let owner = builder.r#if(condition, &[], SourceSpan::UNKNOWN)?;
        let regions = [
            owner.borrow().then_body().as_region_ref(),
            owner.borrow().else_body().as_region_ref(),
        ];
        for (i, region) in regions.into_iter().enumerate() {
            let block = builder.create_block_in_region(region);
            builder.switch_to_block(block);
            record(BuiltinOpBuilder::builder_mut(&mut builder), value, i as u32 + 1)?;
            builder.r#yield([], SourceSpan::UNKNOWN)?;
        }
        builder.switch_to_block(entry);
        removed = record(BuiltinOpBuilder::builder_mut(&mut builder), value, 3)?;
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let root = test.function().as_operation_ref();
    let phase = CaptureInvalidationPhase::begin(root, Some(Rc::new(NoopRewriterListener)))?;
    let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
    invalidate_debug_captures(removed, &mut rewriter)?;
    phase.finish()?;
    assert_eq!(lines(root), [1, 2, 3]);
    Ok(())
}

#[test]
fn structured_optional_else_preserves_both_first_clear_frontiers() -> Result<(), Report> {
    use midenc_hir::{
        EntityMut, Op, OperationName,
        dialects::builtin::Function,
        pass::{Pass, PassExecutionState},
    };

    struct ClearCaptures([OperationRef; 2]);
    impl ClearCaptures {
        fn run(&self, root: OperationRef) -> Result<(), Report> {
            let phase = CaptureInvalidationPhase::begin(root, Some(Rc::new(NoopRewriterListener)))?;
            let mut rewriter: RewriterImpl = RewriterImpl::new(root.borrow().context_rc());
            for capture in self.0 {
                invalidate_debug_captures(capture, &mut rewriter)?;
            }
            phase.finish()?;
            Ok(())
        }
    }
    impl Pass for ClearCaptures {
        type Target = Function;

        fn name(&self) -> &'static str {
            "clear-optional-else-captures"
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
            drop(op);
            self.run(root)
        }
    }

    for audited in [false, true] {
        let mut test = Test::new("optional_else_captures", &[Type::I1, Type::U32], &[Type::U32]);
        let (owner, then_block, removed);
        {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let condition = entry.borrow().arguments()[0] as ValueRef;
            let value = entry.borrow().arguments()[1] as ValueRef;
            record(BuiltinOpBuilder::builder_mut(&mut builder), value, 1)?;
            owner = builder.r#if(condition, &[], SourceSpan::UNKNOWN)?;
            let then_region = owner.borrow().then_body().as_region_ref();
            then_block = builder.create_block_in_region(then_region);
            builder.switch_to_block(then_block);
            let inside = record(BuiltinOpBuilder::builder_mut(&mut builder), value, 2)?;
            builder.r#yield([], SourceSpan::UNKNOWN)?;
            builder.switch_to_block(entry);
            let after = record(BuiltinOpBuilder::builder_mut(&mut builder), value, 3)?;
            builder.ret([value], SourceSpan::UNKNOWN)?;
            removed = [inside, after];
        }
        let root = test.function().as_operation_ref();
        let pass = ClearCaptures(removed);
        if audited {
            test.apply_boxed_pass(alloc::boxed::Box::new(pass), true)?;
        } else {
            pass.run(root)?;
        }
        assert!(owner.borrow().else_body().is_empty());
        assert_eq!(lines(owner.as_operation_ref()), [1, 2], "then frontier, audited={audited}");
        assert_eq!(lines(root), [1, 2, 1, 3], "bypass frontier, audited={audited}");
        assert_eq!(then_block.borrow().body().len(), 3);
    }
    Ok(())
}

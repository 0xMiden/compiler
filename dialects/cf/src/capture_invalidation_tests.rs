use alloc::{rc::Rc, vec::Vec};

use midenc_hir::{
    OperationRef, Report, SourceSpan, Type, ValueRef,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::{
            DIBuilder, DebugKill,
            attributes::Variable,
            debug_record_variable,
            transform::{CaptureInvalidationPhase, collect_debug_ops, invalidate_debug_captures},
        },
    },
    patterns::{NoopRewriterListener, RewriterImpl},
    testing::Test,
};

use crate::ControlFlowOpBuilder;

#[test]
fn capture_flow_handles_cfg_joins_and_backedges() -> Result<(), Report> {
    for cyclic in [false, true] {
        let mut test = Test::new("capture_cfg", &[Type::I1, Type::U32], &[Type::U32]);
        let removed;
        {
            let mut builder = test.function_builder();
            let entry = builder.entry_block();
            let region = entry.parent().unwrap();
            let condition = entry.borrow().arguments()[0] as ValueRef;
            let value = entry.borrow().arguments()[1] as ValueRef;
            let left = builder.create_block_in_region(region);
            let right = builder.create_block_in_region(region);
            let exit = builder.create_block_in_region(region);
            builder.switch_to_block(entry);
            if cyclic {
                builder.br(left, [], SourceSpan::UNKNOWN)?;
            } else {
                builder.cond_br(condition, left, [], right, [], SourceSpan::UNKNOWN)?;
            }
            builder.switch_to_block(left);
            let first = BuiltinOpBuilder::builder_mut(&mut builder)
                .debug_value(
                    value,
                    Variable::new("x".into(), "test.rs".into(), if cyclic { 3 } else { 1 }, None),
                    SourceSpan::UNKNOWN,
                )?
                .as_operation_ref();
            builder.br(if cyclic { right } else { exit }, [], SourceSpan::UNKNOWN)?;
            builder.switch_to_block(right);
            BuiltinOpBuilder::builder_mut(&mut builder).debug_value(
                value,
                Variable::new("x".into(), "test.rs".into(), 2, None),
                SourceSpan::UNKNOWN,
            )?;
            if cyclic {
                builder.cond_br(condition, left, [], exit, [], SourceSpan::UNKNOWN)?;
            } else {
                builder.br(exit, [], SourceSpan::UNKNOWN)?;
            }
            builder.switch_to_block(exit);
            removed = if cyclic {
                first
            } else {
                BuiltinOpBuilder::builder_mut(&mut builder)
                    .debug_value(
                        value,
                        Variable::new("x".into(), "test.rs".into(), 3, None),
                        SourceSpan::UNKNOWN,
                    )?
                    .as_operation_ref()
            };
            builder.ret([value], SourceSpan::UNKNOWN)?;
        }
        let root = test.function().as_operation_ref();
        let phase = CaptureInvalidationPhase::begin(root, Some(Rc::new(NoopRewriterListener)))?;
        let mut rewriter: RewriterImpl = RewriterImpl::new(test.context_rc());
        invalidate_debug_captures(removed, &mut rewriter)?;
        phase.finish()?;
        let kills = collect_debug_ops(&root)
            .into_iter()
            .filter(|op: &OperationRef| op.name().is::<DebugKill>())
            .map(|op| debug_record_variable(&op.borrow()).unwrap().line)
            .collect::<Vec<_>>();
        assert_eq!(
            kills,
            if cyclic {
                alloc::vec![2, 3]
            } else {
                alloc::vec![1, 2, 3]
            }
        );
    }
    Ok(())
}

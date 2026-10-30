use midenc_dialect_scf::StructuredControlFlowOpBuilder;
use midenc_hir::{
    SourceSpan, Type,
    dialects::{
        builtin::BuiltinOpBuilder,
        debuginfo::{DIBuilder, attributes::Variable},
    },
    testing::Test,
};

use super::*;

#[test]
fn pending_anchor_uses_cse_invalidation_fallback() -> Result<(), Report> {
    let mut test = Test::new("pending_cse", &[Type::I1, Type::U32], &[Type::U32]);
    let mut owners = Vec::new();
    let mut captures = Vec::new();
    {
        let mut builder = test.function_builder();
        let entry = builder.entry_block();
        let condition = entry.borrow().arguments()[0] as ValueRef;
        let value = entry.borrow().arguments()[1] as ValueRef;
        for pending in [false, true] {
            builder.switch_to_block(entry);
            let owner = builder.r#if(condition, &[], SourceSpan::UNKNOWN)?;
            let regions = [
                owner.borrow().then_body().as_region_ref(),
                owner.borrow().else_body().as_region_ref(),
            ];
            for region in regions {
                let block = builder.create_block_in_region(region);
                builder.switch_to_block(block);
                let variable = Variable::new("x".into(), "test.rs".into(), 2, None);
                if pending {
                    captures.push(
                        BuiltinOpBuilder::builder_mut(&mut builder)
                            .debug_value(value, variable, SourceSpan::UNKNOWN)?
                            .as_operation_ref(),
                    );
                } else {
                    BuiltinOpBuilder::builder_mut(&mut builder)
                        .debug_kill(variable, SourceSpan::UNKNOWN)?;
                }
                builder.r#yield([], SourceSpan::UNKNOWN)?;
            }
            owners.push(owner.as_operation_ref());
        }
        builder.switch_to_block(entry);
        builder.ret([value], SourceSpan::UNKNOWN)?;
    }
    let root = test.function().as_operation_ref();
    let listener = Rc::new(TracingRewriterListener);
    let phase = CaptureInvalidationPhase::begin(root, Some(listener.clone()))?;
    let mut rewriter =
        RewriterImpl::<TracingRewriterListener>::new(test.context_rc()).with_listener(listener);
    for record in captures {
        invalidate_debug_captures(record, &mut rewriter)?;
    }
    assert!(corresponding_captures(owners[1], owners[0]).is_some());
    let mut driver = CSEDriver {
        rewriter: &mut rewriter,
        domtree: Rc::new(DominanceInfo::new(&root.borrow())),
        ops_to_erase: Vec::new(),
        mem_effects_cache: Default::default(),
    };
    driver.resolve_captures(owners[1], owners[0], true)?;
    assert!(collect_debug_ops(&owners[1]).is_empty());
    assert!(is_pending_capture(owners[1].prev().unwrap()));
    phase.finish()?;
    Ok(())
}

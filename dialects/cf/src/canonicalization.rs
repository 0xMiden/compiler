mod simplify_br_to_block_with_single_pred;
mod simplify_br_to_return;
mod simplify_cond_br_like_switch;
mod simplify_passthrough_br;
mod simplify_passthrough_cond_br;
mod simplify_successor_arguments;
mod simplify_switch_fallback_overlap;
mod split_critical_edges;

pub use self::{
    simplify_br_to_block_with_single_pred::SimplifyBrToBlockWithSinglePred,
    simplify_br_to_return::SimplifyBrToReturn,
    simplify_cond_br_like_switch::SimplifyCondBrLikeSwitch,
    simplify_passthrough_br::SimplifyPassthroughBr,
    simplify_passthrough_cond_br::SimplifyPassthroughCondBr,
    simplify_successor_arguments::RemoveUnusedSinglePredBlockArgs,
    simplify_switch_fallback_overlap::SimplifySwitchFallbackOverlap,
    split_critical_edges::SplitCriticalEdges,
};

#[cfg(test)]
pub(crate) mod testing {
    use alloc::{boxed::Box, rc::Rc};

    use midenc_hir::{
        Context, Listener, ListenerType, OperationRef, ProgramPoint, Report,
        dialects::builtin::FunctionRef,
        patterns::{
            self, FrozenRewritePatternSet, GreedyRewriteConfig, RewritePattern, RewritePatternSet,
            RewriterListener,
        },
    };

    /// Borrows every op of the region around an op it is notified about, as a verifying listener
    /// does, so a pattern that still holds one of those ops while it notifies fails here.
    pub(crate) struct BorrowingListener;

    impl BorrowingListener {
        fn borrow_region_of(op: OperationRef) {
            let Some(region) = op.parent_region() else {
                return;
            };
            let region = region.borrow();
            for block in region.body().iter() {
                for op in block.body().iter() {
                    let _ = op.as_operation_ref().borrow();
                }
            }
        }
    }

    impl Listener for BorrowingListener {
        fn kind(&self) -> ListenerType {
            ListenerType::Rewriter
        }

        fn notify_operation_inserted(&self, op: OperationRef, _prev: ProgramPoint) {
            Self::borrow_region_of(op);
        }
    }

    impl RewriterListener for BorrowingListener {
        fn notify_operation_modified(&self, op: OperationRef) {
            Self::borrow_region_of(op);
        }
    }

    /// Applies `pattern` to `function` with the greedy driver and a [BorrowingListener] attached,
    /// returning whether the function changed
    pub(crate) fn apply_with_borrowing_listener(
        context: &Rc<Context>,
        function: FunctionRef,
        pattern: Box<dyn RewritePattern>,
    ) -> Result<bool, Report> {
        let pattern_set = RewritePatternSet::from_iter(context.clone(), [pattern]);
        let rewrites = Rc::new(FrozenRewritePatternSet::new(pattern_set));
        patterns::apply_patterns_and_fold_greedily(
            function.as_operation_ref(),
            rewrites,
            GreedyRewriteConfig::new_with_listener(BorrowingListener),
        )
        .map_err(|_| Report::msg("the greedy rewrite did not converge"))
    }
}

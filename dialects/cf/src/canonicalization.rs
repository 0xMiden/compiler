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

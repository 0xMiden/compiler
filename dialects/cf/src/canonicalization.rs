mod simplify_br_to_block_with_single_pred;
mod simplify_passthrough_br;
mod simplify_passthrough_cond_br;
mod split_critical_edges;

pub use self::{
    simplify_br_to_block_with_single_pred::SimplifyBrToBlockWithSinglePred,
    simplify_passthrough_br::SimplifyPassthroughBr,
    simplify_passthrough_cond_br::SimplifyPassthroughCondBr,
    split_critical_edges::SplitCriticalEdges,
};

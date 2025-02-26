//mod convert_do_while_to_while_true;
mod convert_trivial_if_to_select;
mod fold_constant_index_switch;
mod if_remove_unused_results;
mod remove_loop_invariant_args_from_before_block;
//mod remove_loop_invariant_value_yielded;
mod simplify_br_to_block_with_single_pred;
mod simplify_passthrough_br;
mod simplify_passthrough_cond_br;
mod while_condition_truth;
mod while_remove_duplicated_results;
mod while_remove_unused_args;
mod while_unused_result;

pub use self::{
    //convert_do_while_to_while_true::ConvertDoWhileToWhileTrue,
    convert_trivial_if_to_select::ConvertTrivialIfToSelect,
    fold_constant_index_switch::FoldConstantIndexSwitch,
    if_remove_unused_results::IfRemoveUnusedResults,
    remove_loop_invariant_args_from_before_block::RemoveLoopInvariantArgsFromBeforeBlock,
    simplify_br_to_block_with_single_pred::SimplifyBrToBlockWithSinglePred,
    simplify_passthrough_br::SimplifyPassthroughBr,
    simplify_passthrough_cond_br::SimplifyPassthroughCondBr,
    while_condition_truth::WhileConditionTruth,
    while_remove_duplicated_results::WhileRemoveDuplicatedResults,
    while_remove_unused_args::WhileRemoveUnusedArgs,
    while_unused_result::WhileUnusedResult,
};

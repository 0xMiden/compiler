use std::collections::VecDeque;

use midenc_hir::{
    self as hir,
    pass::{AnalysisManager, RewritePass, RewriteResult},
    *,
};
use midenc_hir_analysis::ControlFlowGraph;
use midenc_session::{diagnostics::IntoDiagnostic, Session};
use smallvec::SmallVec;

use crate::adt::ScopedMap;

/// This pass inlines unnecessary blocks, and removes unnecessary block arguments.
///
/// Specifically, the blocks affected are those with a single predecessor, as they
/// by definition can be inlined into their immediate predecessor in such cases.
///
/// Blocks like this may have been introduced for the following reasons:
///
/// * Due to less than optimal lowering to SSA form
/// * To split critical edges in preparation for dataflow analysis and related transformations,
///   but ultimately no code introduced along those edges, and critical edges no longer present
///   an obstacle to further optimization or codegen.
/// * During treeification of the CFG, where blocks with multiple predecessors were duplicated
///   to produce a CFG in tree form, where no blocks (other than loop headers) have multiple
///   predecessors. This process removed block arguments from these blocks, and rewrote instructions
///   dominated by those block arguments to reference the values passed from the original
///   predecessor to whom the subtree is attached. This transformation can expose a chain of blocks
///   which all have a single predecessor and successor, introducing branches where none are needed,
///   and by removing those redundant branches, all of the code from blocks in the chain can be
///   inlined in the first block of the chain.
#[derive(Default, PassInfo, ModuleRewritePassAdapter)]
pub struct InlineBlocks;
impl RewritePass for InlineBlocks {
    type Entity = hir::Function;

    fn apply(
        &mut self,
        function: &mut Self::Entity,
        analyses: &mut AnalysisManager,
        session: &Session,
    ) -> RewriteResult {
        let mut cfg = analyses
            .take::<ControlFlowGraph>(&function.id)
            .unwrap_or_else(|| ControlFlowGraph::with_function(function));

        let entry = function.dfg.entry_block();
        let mut changed = false;
        let mut rewrites = ScopedMap::<Value, Value>::default();
        let mut visited = FxHashSet::<Block>::default();
        let mut worklist = VecDeque::<Block>::default();
        worklist.push_back(entry);

        // First, search down the CFG for non-loop header blocks with only a single successor.
        // These blocks form possible roots of a chain of blocks that can be inlined.
        //
        // For each such root, we then check if the successor block has a single predecessor,
        // if so, then we can remove the terminator instruction from the root block, and then
        // move all of the code from the successor block into the root block. We can then repeat
        // this process until we inline a terminator instruction that is not an unconditional branch
        // to a single successor.
        while let Some(p) = worklist.pop_front() {
            // If we've already visited a block, skip it
            if !visited.insert(p) {
                continue;
            }

            // If the block is detached, then it has already been inlined,
            // so we do not need to run inlining on it, however we should
            // visit its successors anyway, as there may be inlining
            // opportunities later in the CFG.
            let is_detached = !function.dfg.is_block_linked(p);

            // If this block has multiple successors, or multiple predecessors, add all of it's
            // successors to the work queue and move on.
            if is_detached || cfg.num_successors(p) > 1 || cfg.num_predecessors(p) > 1 {
                for b in cfg.succ_iter(p) {
                    worklist.push_back(b);
                }
                continue;
            }

            // This block is a candidate for inlining
            //
            // If inlining can proceed, do so until we reach a point where the inlined terminator
            // returns from the function, has multiple successors, or branches to a block with
            // multiple predecessors.
            while let BranchInfo::SingleDest(succ) =
                function.dfg.analyze_branch(function.dfg.last_inst(p).unwrap())
            {
                let destination = succ.destination;

                // If this successor has other predecessors, it can't be inlined, so
                // add it to the work list and move on
                if cfg.num_predecessors(destination) > 1 {
                    worklist.push_back(destination);
                    break;
                }

                // Only inline if the successor has no block arguments
                //
                // TODO: We can inline blocks with arguments as well, but with higher cost,
                // as we must visit all uses of the block arguments and update them. This
                // is left as a future extension of this pass should we find that it is
                // valuable as an optimization.
                if !succ.args.is_empty() {
                    // Compute the set of values to rewrite
                    for (from, to) in function
                        .dfg
                        .block_params(destination)
                        .iter()
                        .copied()
                        .zip(succ.args.iter().copied())
                    {
                        rewrites.insert(from, to);
                    }
                }

                inline(destination, p, function, &mut worklist, &rewrites, &mut cfg);

                // Mark that the control flow graph as modified
                changed = true;
            }
        }

        rewrite_uses(entry, function, &rewrites);

        analyses.insert(function.id, cfg);
        if !changed {
            analyses.mark_preserved::<ControlFlowGraph>(&function.id);
        }

        session.print(&*function, Self::FLAG).into_diagnostic()?;
        if session.should_print_cfg(Self::FLAG) {
            use std::io::Write;
            let cfg = function.cfg_printer();
            let mut stdout = std::io::stdout().lock();
            write!(&mut stdout, "{cfg}").into_diagnostic()?;
        }

        Ok(())
    }
}

fn inline(
    from: Block,
    to: Block,
    function: &mut hir::Function,
    worklist: &mut VecDeque<Block>,
    rewrites: &ScopedMap<Value, Value>,
    cfg: &mut ControlFlowGraph,
) {
    assert_ne!(from, to);
    let mut from_terminator = None;
    {
        let mut from_insts = function.dfg.block_mut(from).insts.take();
        let to_insts = &mut function.dfg.blocks[to].insts;
        // Remove the original terminator
        to_insts.pop_back();
        // Move all instructions from their original block to the parent,
        // updating the instruction data along the way to reflect the change
        // in location
        while let Some(unsafe_ix_ref) = from_insts.pop_front() {
            let mut ix = unsafe { UnsafeRef::into_box(unsafe_ix_ref) };
            ix.block = to;
            rewrite_use(ix.as_mut(), &mut function.dfg.value_lists, rewrites);
            // We need to clone the original terminator so we can continue to
            // navigate the control flow graph
            if ix.opcode().is_terminator() {
                let replacement = Box::new(ix.deep_clone(&mut function.dfg.value_lists));
                assert!(
                    from_terminator.replace(replacement).is_none(),
                    "a block can only have one terminator"
                );
            }
            to_insts.push_back(UnsafeRef::from_box(ix));
        }
    }
    // Append the cloned terminator back to the inlined block before we detach it
    let from_terminator = from_terminator.expect("a block must have a terminator");
    match (*from_terminator).as_ref() {
        Instruction::Br(Br { successor, .. }) => {
            worklist.push_back(successor.destination);
        }
        Instruction::CondBr(CondBr {
            then_dest,
            else_dest,
            ..
        }) => {
            worklist.push_back(then_dest.destination);
            worklist.push_back(else_dest.destination);
        }
        Instruction::Switch(hir::Switch {
            arms,
            default: default_dest,
            ..
        }) => {
            worklist.extend(arms.iter().map(|arm| arm.successor.destination));
            worklist.push_back(default_dest.destination);
        }
        _ => (),
    }
    function.dfg.blocks[from].insts.push_back(UnsafeRef::from_box(from_terminator));
    // Detach the original block from the function
    function.dfg.detach_block(from);
    // Update the control flow graph to reflect the changes
    cfg.detach_block(from);
    cfg.recompute_block(&function.dfg, to);
}

fn rewrite_uses(root: Block, function: &mut hir::Function, rewrites: &ScopedMap<Value, Value>) {
    let mut visited = FxHashSet::<Block>::default();
    let mut worklist = VecDeque::<Block>::default();
    worklist.push_back(root);

    while let Some(b) = worklist.pop_front() {
        // Do not visit the same block twice
        if !visited.insert(b) {
            continue;
        }

        let block = &mut function.dfg.blocks[b];
        // Take the list of instructions away from the block to simplify traversing the block
        let mut insts = block.insts.take();
        // Take each instruction out of the list, top to bottom, modify it, then
        // add it back to the instruction list of the block directly. This ensures
        // we traverse the list and rewrite instructions in a single pass without
        // any additional overhead
        while let Some(inst) = insts.pop_front() {
            let mut inst = unsafe { UnsafeRef::into_box(inst) };
            let to_visit = rewrite_use(inst.as_mut(), &mut function.dfg.value_lists, rewrites);
            if !to_visit.is_empty() {
                worklist.extend(to_visit);
            }

            block.insts.push_back(UnsafeRef::from_box(inst));
        }
    }
}

fn rewrite_use(
    inst: &mut Instruction,
    pool: &mut hir::ValueListPool,
    rewrites: &ScopedMap<Value, Value>,
) -> SmallVec<[Block; 2]> {
    let mut worklist = SmallVec::<[Block; 2]>::default();
    match inst {
        Instruction::Br(Br {
            ref mut successor, ..
        }) => {
            worklist.push(successor.destination);
            for arg in successor.args.as_mut_slice(pool) {
                if let Some(replacement) = rewrites.get(arg).copied() {
                    *arg = replacement;
                }
            }
        }
        Instruction::CondBr(CondBr {
            ref mut cond,
            ref mut then_dest,
            ref mut else_dest,
            ..
        }) => {
            worklist.push(then_dest.destination);
            worklist.push(else_dest.destination);
            if let Some(replacement) = rewrites.get(cond).copied() {
                *cond = replacement;
            }
            for arg in then_dest.args.as_mut_slice(pool) {
                if let Some(replacement) = rewrites.get(arg).copied() {
                    *arg = replacement;
                }
            }
            for arg in else_dest.args.as_mut_slice(pool) {
                if let Some(replacement) = rewrites.get(arg).copied() {
                    *arg = replacement;
                }
            }
        }
        Instruction::Switch(hir::Switch {
            ref mut arg,
            ref mut arms,
            default: ref mut default_dest,
            ..
        }) => {
            worklist.extend(arms.iter().map(|arm| arm.successor.destination));
            worklist.push(default_dest.destination);
            if let Some(replacement) = rewrites.get(arg).copied() {
                *arg = replacement;
            }
            for arg in default_dest.args.as_mut_slice(pool) {
                if let Some(replacement) = rewrites.get(arg).copied() {
                    *arg = replacement;
                }
            }
            for arm in arms.iter_mut() {
                for arg in arm.successor.args.as_mut_slice(pool) {
                    if let Some(replacement) = rewrites.get(arg).copied() {
                        *arg = replacement;
                    }
                }
            }
        }
        op => {
            for arg in op.arguments_mut(pool) {
                if let Some(replacement) = rewrites.get(arg).copied() {
                    *arg = replacement;
                }
            }
        }
    }

    worklist
}

#[cfg(test)]
mod tests {
    use midenc_hir::{
        pass::{AnalysisManager, RewritePass},
        testing::TestContext,
        AbiParam, Function, FunctionBuilder, Immediate, InstBuilder, Signature, SourceSpan, Type,
    };
    use pretty_assertions::{assert_eq, assert_ne};

    use crate::InlineBlocks;

    /// Run the inlining pass on the following IR:
    ///
    /// The following IR is unnecessarily verbose:
    ///
    /// ```text,ignore
    /// pub fn test(*mut u8, i32) -> *mut u8 {
    /// entry(ptr0: *mut u8, offset: i32):
    ///    zero = const.u32 0;
    ///    ptr1 = ptrtoint ptr0 : u32;
    ///    is_null = eq ptr1, zero;
    ///    br blk0(ptr1, is_null);
    ///
    /// blk0(ptr2: u32, is_null1: i1):
    ///    condbr is_null1, blk2(ptr0), blk1(ptr2);
    ///
    /// blk1(ptr3: u32):
    ///    ptr4 = add ptr3, offset;
    ///    is_null2 = eq ptr4, zero;
    ///    condbr is_null2, blk4(ptr0), blk5(ptr4);
    ///
    /// blk2(result0: *mut u8):
    ///    br blk3;
    ///
    /// blk3:
    ///    ret result0;
    ///
    /// blk4(result1: *mut u8):
    ///    ret result1;
    ///
    /// blk5(ptr5: u32):
    ///    ptr6 = inttoptr ptr5 : *mut u8;
    ///    ret ptr6;
    /// }
    /// ```
    ///
    /// We want the inlining pass to result in something like:
    ///
    /// ```text,ignore
    /// pub fn test(*mut u8, i32) -> *mut u8 {
    /// entry(ptr0: *mut u8, offset: i32):
    ///   zero = const.u32 0;
    ///   ptr1 = ptrtoint ptr0 : u32;
    ///   is_null = eq ptr1, zero;
    ///   condbr is_null, blk2, blk1;
    ///
    /// blk1:
    ///   ptr2 = add ptr1, offset;
    ///   is_null1 = eq ptr2, zero;
    ///   condbr is_null1, blk3, blk4;
    ///
    /// blk2:
    ///   ret ptr0;
    ///
    /// blk3:
    ///   ret ptr0;
    ///
    /// blk4:
    ///   ptr3 = inttoptr ptr2 : *mut u8;
    ///   ret ptr3;
    /// }
    /// ```
    ///
    /// In short, regardless of block arguments, control flow edges between blocks
    /// where the predecessor is the only predecessor, and the successor is the only
    /// successor, represent edges which can be removed by inlining the successor
    /// block into the predecessor block. Any uses of values introduced as block
    /// parameters of the successor block, must be rewritten to use the values
    /// provided in the predecessor block for those parameters.
    #[test]
    fn inline_blocks_simple_tree_cfg_test() {
        let context = TestContext::default();
        let id = "test::inlining_test".parse().unwrap();
        let mut function = Function::new(
            id,
            Signature::new(
                [AbiParam::new(Type::Ptr(Box::new(Type::U8))), AbiParam::new(Type::I32)],
                [AbiParam::new(Type::Ptr(Box::new(Type::U8)))],
            ),
        );

        {
            let mut builder = FunctionBuilder::new(&mut function);
            let entry = builder.current_block();
            let (ptr0, offset) = {
                let args = builder.block_params(entry);
                (args[0], args[1])
            };

            let a = builder.create_block(); // blk0(ptr2: u32, is_null1: i1)
            let ptr2 = builder.append_block_param(a, Type::U32, SourceSpan::UNKNOWN);
            let is_null1 = builder.append_block_param(a, Type::I1, SourceSpan::UNKNOWN);
            let b = builder.create_block(); // blk1(ptr3: u32)
            let ptr3 = builder.append_block_param(b, Type::U32, SourceSpan::UNKNOWN);
            let c = builder.create_block(); // blk2(result0: *mut u8)
            let result0 =
                builder.append_block_param(c, Type::Ptr(Box::new(Type::U8)), SourceSpan::UNKNOWN);
            let d = builder.create_block(); // blk3
            let e = builder.create_block(); // blk4(result1: *mut u8)
            let result1 =
                builder.append_block_param(e, Type::Ptr(Box::new(Type::U8)), SourceSpan::UNKNOWN);
            let f = builder.create_block(); // blk5(ptr5: u32)
            let ptr5 = builder.append_block_param(f, Type::U32, SourceSpan::UNKNOWN);

            // entry
            let ptr1 = builder.ins().ptrtoint(ptr0, Type::U32, SourceSpan::UNKNOWN);
            let is_null = builder.ins().eq_imm(ptr1, Immediate::U32(0), SourceSpan::UNKNOWN);
            builder.ins().br(a, &[ptr1, is_null], SourceSpan::UNKNOWN);

            // blk0
            builder.switch_to_block(a);
            builder.ins().cond_br(is_null1, c, &[ptr0], b, &[ptr2], SourceSpan::UNKNOWN);

            // blk1
            builder.switch_to_block(b);
            let ptr3_i32 = builder.ins().cast(ptr3, Type::I32, SourceSpan::UNKNOWN);
            let ptr4_i32 = builder.ins().add_checked(ptr3_i32, offset, SourceSpan::UNKNOWN);
            let ptr4 = builder.ins().cast(ptr4_i32, Type::U32, SourceSpan::UNKNOWN);
            let is_null2 = builder.ins().eq_imm(ptr4, Immediate::U32(0), SourceSpan::UNKNOWN);
            builder.ins().cond_br(is_null2, e, &[ptr0], f, &[ptr4], SourceSpan::UNKNOWN);

            // blk2
            builder.switch_to_block(c);
            builder.ins().br(d, &[], SourceSpan::UNKNOWN);

            // blk3
            builder.switch_to_block(d);
            builder.ins().ret(Some(result0), SourceSpan::UNKNOWN);

            // blk4
            builder.switch_to_block(e);
            builder.ins().ret(Some(result1), SourceSpan::UNKNOWN);

            // blk5
            builder.switch_to_block(f);
            let ptr6 =
                builder.ins().inttoptr(ptr5, Type::Ptr(Box::new(Type::U8)), SourceSpan::UNKNOWN);
            builder.ins().ret(Some(ptr6), SourceSpan::UNKNOWN);
        }

        let original = function.to_string();
        let mut analyses = AnalysisManager::default();
        let mut rewrite = InlineBlocks;
        rewrite
            .apply(&mut function, &mut analyses, &context.session)
            .expect("inlining failed");

        let expected = "\
(func (export #inlining_test) (param (ptr u8)) (param i32) (result (ptr u8))
    (block 0 (param v0 (ptr u8)) (param v1 i32)
        (let (v8 u32) (ptrtoint v0))
        (let (v9 i1) (eq v8 0))
        (condbr v9 (block 3 v0) (block 2 v8)))

    (block 2 (param v4 u32)
        (let (v10 i32) (cast v4))
        (let (v11 i32) (add.checked v10 v1))
        (let (v12 u32) (cast v11))
        (let (v13 i1) (eq v12 0))
        (condbr v13 (block 5 v0) (block 6 v12)))

    (block 3 (param v5 (ptr u8))
        (ret v5))

    (block 5 (param v6 (ptr u8))
        (ret v6))

    (block 6 (param v7 u32)
        (let (v14 (ptr u8)) (inttoptr v7))
        (ret v14))
)";

        let inlined = function.to_string();
        assert_ne!(inlined, original);
        assert_eq!(inlined.as_str(), expected);
    }
}
